use crate::config::Config;
use anyhow::Context;
use common::{
    domain::{machine_status::IpConnection, Hostname, MacAddr, MachineStatus},
    net::{auth_client::UrlParseError, AuthenticatedClient},
};
use futures::stream::{self, StreamExt, TryStreamExt};
use pnet::datalink;
use reqwest::StatusCode;
use std::{
    convert::{Infallible, TryFrom},
    error::Error,
    io,
    mem::take,
    net::IpAddr,
    str::FromStr,
    time::Duration,
};
use tokio::{process::Command, time::sleep};
use tracing::{debug, error, info_span, warn};

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
            Ok(IpAddr::from_str(string.trim()).map_err(to_io_e)?)
        });
    match dig {
        Ok(dig) => Ok(dig),
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                warn!("consider installing dig for better performance");
            }
            Ok(IpAddr::from_str(
                &reqwest::get("https://ifconfig.me").await?.text().await?,
            )?)
        }
    }
}

async fn gateway_ip_and_mac() -> anyhow::Result<(IpAddr, Option<MacAddr>)> {
    let mut out = Command::new("sh")
        .args(["-c", "ip route | grep default | awk '{print $3}'"])
        .output()
        .await?;

    if out.status.success() {
        let ip_str = String::from_utf8(take(&mut out.stdout))?;
        let ip_str = ip_str.trim();
        let ip =
            IpAddr::from_str(&ip_str).with_context(|| format!("tried to parse: {:?}", ip_str))?;
        let mut out = Command::new("sh")
            .args([
                "-c",
                &format!("ip neigh | grep '{} ' | awk '{{ print $5 }}'", ip_str),
            ])
            .output()
            .await?;
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

pub async fn get_ip_connections() -> anyhow::Result<Vec<IpConnection>> {
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
        let (gateway_ip, gateway_mac) = gateway_ip_and_mac().await?;
        Ok(IpConnection {
            local_ip: network.ip(),
            gateway_ip,
            gateway_mac,
        })
    })
    .try_collect()
    .await
}

pub async fn get_hostname() -> anyhow::Result<Hostname> {
    tokio::task::spawn_blocking(|| {
        Ok(Hostname::try_from(
            hostname::get()?.to_string_lossy().into_owned(),
        )?)
    })
    .await?
}

async fn get_current_status() -> anyhow::Result<MachineStatus> {
    let (hostname, ip_connections, external_ip) =
        tokio::try_join!(get_hostname(), get_ip_connections(), get_external_ip())?;

    Ok(MachineStatus {
        hostname,
        ip_connections,
        external_ip,
    })
}

pub async fn start(config: &Config) -> Result<Infallible, UrlParseError> {
    let client = AuthenticatedClient::new(config.token.clone(), &config.backend_url)?;
    loop {
        let _span = info_span!("post machine status");
        match get_current_status().await {
            Ok(status) => {
                let result = client
                    .post("/machine/status")
                    .expect("building a request")
                    .json(&status)
                    .send()
                    .await;
                match result {
                    Ok(r) if r.status() == StatusCode::OK => debug!("Post succeeded"),
                    Ok(r) => error!("Post request failed: {}", r.status()),
                    Err(e) => error!("Network request failed: {:?}", e),
                }
            }
            Err(e) => error!("Failed to obtain a machine status: {:?}", e),
        };

        sleep(Duration::from_secs(60)).await;
    }
}
