pub mod destination;

use std::{mem::take, net::IpAddr, pin::pin, str::FromStr};

use anyhow::Context;
use common::domain::{machine_status::IpConnection, Hostname, MacAddr, MachineStatus};
use futures::{
    future::{select, Either},
    stream, StreamExt, TryStreamExt,
};
use pnet::datalink;
use tokio::process::Command;

use crate::config::Config;

pub(crate) async fn get_hostname() -> anyhow::Result<Hostname> {
    Ok(tokio::task::spawn_blocking(Hostname::from_this_host).await??)
}

async fn get_external_ip() -> anyhow::Result<IpAddr> {
    match select(pin!(public_ip::addr_v4()), pin!(public_ip::addr_v6())).await {
        Either::Left((Some(v4), _)) => Some(IpAddr::V4(v4)),
        Either::Left((None, backup)) => backup.await.map(IpAddr::V6),
        Either::Right((backup, main)) => main.await.map(IpAddr::V4).or(backup.map(IpAddr::V6)),
    }
    .ok_or_else(|| anyhow::anyhow!("failed to get external ip"))
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
