pub mod destination;

use std::{net::IpAddr, pin::pin};

use anyhow::Context;
use common::domain::{
    Hostname, MacAddr, MachineStatus, mac::MacAddr6, machine_status::IpConnection,
};
use futures::future::{Either, select};

use crate::config::Config;
use default_net::interface::InterfaceType;

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
    let (gateway, ips) = tokio::task::spawn_blocking(default_net::get_interfaces)
        .await
        .context("panicked while getting interfaces")?
        .into_iter()
        .inspect(|iface| println!("{}: {:?}", iface.name, iface.gateway))
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
                    .map(|v4| IpAddr::V4(v4.network()))
                    .chain(iface.ipv6.into_iter().map(|v6| IpAddr::V6(v6.network()))),
            );
            (dbg!(dbg!(gateway).or(dbg!(iface.gateway))), ips)
        });
    let Some(gateway) = gateway else {
        anyhow::bail!("no gateway found");
    };
    Ok(ips
        .into_iter()
        .map(|ip| IpConnection {
            local_ip: ip,
            gateway_ip: gateway.ip_addr,
            gateway_mac: Some(MacAddr::V6(MacAddr6(gateway.mac_addr.octets()))),
        })
        .collect())
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
