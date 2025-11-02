pub mod destination;

use std::{net::IpAddr, pin::pin};

use anyhow::Context;
use common::domain::{Hostname, MachineStatus, machine_status::IpConnection};
use futures::future::{Either, select};

use crate::config::Config;
use tokio::process::Command;

#[cfg(not(target_os = "android"))]
use common::domain::{MacAddr, mac::MacAddr6};
#[cfg(not(target_os = "android"))]
use default_net::interface::InterfaceType;

pub(crate) async fn get_hostname(config: &Config) -> anyhow::Result<Hostname> {
    Ok(match &config.hostname_override {
        Some(h) => h.clone(),
        None => tokio::task::spawn_blocking(Hostname::from_this_host).await??,
    })
}

async fn get_external_ip() -> anyhow::Result<IpAddr> {
    match select(pin!(public_ip::addr_v4()), pin!(public_ip::addr_v6())).await {
        Either::Left((Some(v4), _)) => Some(IpAddr::V4(v4)),
        Either::Left((None, backup)) => backup.await.map(IpAddr::V6),
        Either::Right((backup, main)) => main.await.map(IpAddr::V4).or(backup.map(IpAddr::V6)),
    }
    .ok_or_else(|| anyhow::anyhow!("failed to get external ip"))
}

#[cfg(target_os = "android")]
async fn get_ip_connections() -> anyhow::Result<Vec<IpConnection>> {
    let output = Command::new("ifconfig").output().await?;

    fn extract_ip(interface: &str, data: &str) -> Option<(IpAddr, IpAddr)> {
        let mut lines = data.lines().peekable();

        while let Some(line) = lines.next() {
            // match line like "wlan0: flags=..."
            if line
                .strip_prefix(interface)
                .and_then(|s| s.strip_prefix(":"))
                .is_some()
            {
                fn tag<'s>(i: &mut impl Iterator<Item = &'s str>, tag: &str) -> Option<()> {
                    (i.next()? == tag).then_some(())
                }

                fn parse_line(line: &str) -> Option<(IpAddr, IpAddr)> {
                    let mut line = line.split_whitespace();
                    tag(&mut line, "inet")?;
                    let ip = line.next()?.parse::<std::net::Ipv4Addr>().ok()?;
                    tag(&mut line, "netmask")?;
                    let mask = line.next()?.parse::<std::net::Ipv4Addr>().ok()?;
                    let mut gateway_speculation = ip.octets();
                    std::iter::zip(&mut gateway_speculation, mask.octets()).for_each(|(g, m)| {
                        if m == 0 {
                            *g = 1;
                        }
                    });
                    Some((
                        IpAddr::from(ip),
                        IpAddr::from(std::net::Ipv4Addr::from(gateway_speculation)),
                    ))
                }

                // consume following indented lines that belong to this interface
                while let Some(&next) = lines.peek() {
                    if let Some(x) = parse_line(next) {
                        return Some(x);
                    }
                }
            }
        }

        None
    }
    let (ip, gateway_ip) =
        extract_ip("wlan0", std::str::from_utf8(&output.stdout)?).context("dajskldjasd")?;
    Ok(vec![IpConnection {
        local_ip: ip,
        gateway_ip,
        gateway_mac: None,
    }])
}

#[cfg(not(target_os = "android"))]
async fn get_ip_connections() -> anyhow::Result<Vec<IpConnection>> {
    let (gateway, ips) = tokio::task::spawn_blocking(default_net::get_interfaces)
        .await
        .context("panicked while getting interfaces")?
        .into_iter()
        .filter(|iface| !iface.is_loopback())
        .filter(|iface| iface.is_up())
        .filter(|iface| iface.if_type == InterfaceType::Ethernet)
        .filter(|iface| !iface.name.starts_with("docker"))
        .filter(|iface| !iface.name.starts_with("veth"))
        .fold((None, vec![]), |(gateway, mut ips), iface| {
            ips.extend(
                iface
                    .ipv4
                    .into_iter()
                    .map(|v4| IpAddr::V4(v4.addr))
                    .chain(iface.ipv6.into_iter().map(|v6| IpAddr::V6(v6.addr))),
            );
            (gateway.or(iface.gateway), ips)
        });
    let (gateway_ip, gateway_mac) = match gateway {
        Some(g) => (g.ip_addr, Some(MacAddr::V6(MacAddr6(g.mac_addr.octets())))),
        None => match get_gateway_fallback().await {
            Ok(g) => g,
            Err(e) => return Err(e),
        },
    };
    Ok(ips
        .into_iter()
        .map(|ip| IpConnection {
            local_ip: ip,
            gateway_ip,
            gateway_mac,
        })
        .collect())
}

#[cfg(not(target_os = "android"))]
async fn get_gateway_fallback() -> anyhow::Result<(IpAddr, Option<MacAddr>)> {
    let mut out = if cfg!(target_os = "android") {
        return Ok((std::net::Ipv4Addr::LOCALHOST.into(), None));
    } else {
        Command::new("sh").args(["-c", "ip route"]).output().await
    }
    .context("running ip route")?;

    if out.status.success() {
        let routing = String::from_utf8(std::mem::take(&mut out.stdout)).map_err(|e| {
            anyhow::anyhow!(
                "failed to convert {:?} to utf8. Details: {:?}",
                e.as_bytes(),
                e.utf8_error()
            )
        })?;
        let (gateway_ip, _) = routing
            .split("\n")
            .filter(|line| line.starts_with("default"))
            .map(|line| {
                let mut parts = line.split_whitespace();
                let gateway_ip = parts
                    .by_ref()
                    .nth(2)
                    .with_context(|| format!("invalid route table line: {line}"))?
                    .parse::<IpAddr>()
                    .with_context(|| format!("invalid gateway ip: {line}"))?;
                let metric = parts
                    .last()
                    .unwrap()
                    .parse::<usize>()
                    .with_context(|| format!("invalid metric number: {line}"))?;
                Ok((gateway_ip, metric))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .min_by_key(|(_, metric)| *metric)
            .context("routing table is empty")?;
        let mut out = Command::new("sh")
            .args([
                "-c",
                &format!("ip neigh | grep '{gateway_ip} ' | awk '{{ print $5 }}'"),
            ])
            .output()
            .await
            .context("running 'ip neigh'")?;
        if let Some(mac) = out
            .status
            .success()
            .then(|| std::mem::take(&mut out.stdout))
            .and_then(|s| String::from_utf8(s).ok())
            .and_then(|s| s.trim().parse::<MacAddr>().ok())
        {
            Ok((gateway_ip, Some(mac)))
        } else {
            Ok((gateway_ip, None))
        }
    } else {
        Err(anyhow::anyhow!(
            "failed to get ip, exit code: {}, stderr: '{}'",
            out.status,
            String::from_utf8_lossy(&out.stderr),
        ))
    }
}

pub(crate) async fn get_current_status(config: &Config) -> anyhow::Result<MachineStatus> {
    let (hostname, ip_connections, external_ip) = tokio::try_join!(
        async { get_hostname(config).await.context("getting hostname") },
        async { get_ip_connections().await.context("getting ip connections") },
        async { get_external_ip().await },
    )?;

    tracing::debug!(%hostname, %external_ip, "current status obtained");

    Ok(MachineStatus {
        hostname,
        ssh: config.network.ssh,
        ip_connections,
        external_ip,
        default_user: config.default_user.clone().or_else(|| {
            let username = whoami::username();
            (username != "root").then_some(username)
        }),
    })
}
