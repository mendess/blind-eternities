pub mod destination;

use std::{
    error::Error,
    mem::take,
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
};

use anyhow::Context;
use common::domain::{machine_status::IpConnection, Hostname, MacAddr, MachineStatus};
use futures::{stream, StreamExt, TryStreamExt};
use pnet::datalink;
use tokio::{io, process::Command};
use tracing::warn;

use crate::config::Config;

pub(crate) async fn get_hostname() -> anyhow::Result<Hostname> {
    Ok(tokio::task::spawn_blocking(Hostname::from_this_host).await?)
}

async fn get_external_ip() -> anyhow::Result<IpAddr> {
    let dig = Command::new("dig")
        .args([
            "dig",
            "+short",
            "myip.opendns.com",
            "@resolver1.opendns.com",
        ])
        .output()
        .await
        .and_then(|o| {
            fn to_io_e(e: impl Error + Send + Sync + 'static) -> io::Error {
                io::Error::new(io::ErrorKind::Other, e)
            }
            let string = std::str::from_utf8(&o.stdout).map_err(to_io_e)?;
            IpAddr::from_str(string.trim()).map_err(to_io_e)
        });
    match dig {
        Ok(dig) => Ok(dig),
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                warn!("consider installing dig for better performance");
            }
            Ok(IpAddr::from_str(
                &reqwest::Client::builder()
                    .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                    .build()
                    .unwrap()
                    .get("https://ifconfig.me")
                    .send()
                    .await
                    .context("requesting ifconfig.me")?
                    .text()
                    .await
                    .context("parsing ip from ifconfig.me")?,
            )?)
        }
    }
}

async fn get_ip_connections() -> anyhow::Result<Vec<IpConnection>> {
    let (gateway_ip, gateway_mac) = gateway_ip_and_mac().await.context("getting ip and mac")?;
    stream::iter(
        tokio::task::spawn_blocking(|| {
            datalink::interfaces()
                .into_iter()
                .filter(|n| !n.is_loopback())
                .filter(|n| n.is_up())
                .filter(|n| !n.name.starts_with("docker"))
                .filter(|n| !n.name.starts_with("veth"))
                .flat_map(|n| n.ips)
        })
        .await?,
    )
    .then(|network| async move {
        Ok(IpConnection {
            local_ip: network.ip(),
            gateway_ip,
            gateway_mac,
        })
    })
    .try_collect()
    .await
}

async fn gateway_ip_and_mac() -> anyhow::Result<(IpAddr, Option<MacAddr>)> {
    let mut out = if cfg!(target_os = "macos") {
        Command::new("sh")
            .args(["-c", "route -n get default | grep gateway | cut -d: -f2"])
            .output()
            .await
    } else {
        Command::new("sh")
            .args(["-c", "ip route get 1.1.1.1 | awk '{print $3}' | head -1"])
            .output()
            .await
    }
    .context("running ip route")?;

    if out.status.success() {
        let ip_str = String::from_utf8(take(&mut out.stdout)).map_err(|e| {
            anyhow::anyhow!(
                "failed to convert {:?} to utf8. Details: {:?}",
                e.as_bytes(),
                e.utf8_error()
            )
        })?;
        let ip_str = ip_str.trim();
        let ip =
            IpAddr::from_str(ip_str).with_context(|| format!("tried to parse: {:?}", ip_str))?;
        let mut out = Command::new("sh")
            .args([
                "-c",
                &format!("ip neigh | grep '{} ' | awk '{{ print $5 }}'", ip_str),
            ])
            .output()
            .await
            .context("running 'ip neigh'")?;
        if let Some(mac) = out
            .status
            .success()
            .then(|| take(&mut out.stdout))
            .and_then(|s| String::from_utf8(s).ok())
            .and_then(|s| MacAddr::from_str(s.trim()).ok())
        {
            Ok((ip, Some(mac)))
        } else {
            Ok((ip, None))
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
        async { get_hostname().await.context("getting hostname") },
        async { get_ip_connections().await.context("getting ip connections") },
        async { get_external_ip().await.context("getting external ip") },
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
