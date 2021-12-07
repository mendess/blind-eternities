use std::{collections::HashMap, net::IpAddr, path::PathBuf, process::ExitStatus};

use common::{
    algorithms::net_graph::NetGraph,
    domain::{Hostname, MachineStatus},
    net::AuthenticatedClient,
};
use itertools::Itertools;
use structopt::StructOpt;
use tokio::process::Command;
use tracing::{debug, info};

use crate::{config::Config, daemon::machine_status::get_hostname};

#[derive(StructOpt, Debug)]
pub(super) struct SshOpts {
    destination: Hostname,
    #[structopt(short, long)]
    username: Option<String>,
    #[structopt(short, long, default_value = "22")]
    port: u16,
}

pub(super) async fn ssh(opts: SshOpts, config: &'static Config) -> anyhow::Result<ExitStatus> {
    let mut args = route_to_ssh_hops(&opts, config).await?;
    args.push("-p".to_string());
    args.push(opts.port.to_string());
    debug!("running ssh with args {:?}", args);
    Ok(Command::new("ssh").args(args).spawn()?.wait().await?)
}

#[derive(StructOpt, Debug)]
pub(super) struct RsyncOpts {
    rsync_options: String,
    #[structopt(flatten)]
    ssh_opts: SshOpts,
    source_path: PathBuf,
    dest_path: PathBuf,
}

pub(super) async fn rsync(opts: RsyncOpts, config: &'static Config) -> anyhow::Result<ExitStatus> {
    #[allow(unstable_name_collisions)]
    let bridge = route_to_ssh_hops(&opts.ssh_opts, config)
        .await?
        .iter()
        .map(|s| s.as_str())
        .intersperse(" ")
        .fold(String::from("ssh "), |acc, e| acc + e);
    let args = [
        format!("-{}", opts.rsync_options),
        String::from("-e"),
        bridge,
        opts.source_path.to_str().unwrap().to_owned(),
        format!(":{}", opts.dest_path.to_str().unwrap()),
    ];
    debug!("running rsync with args: {:?}", args);
    info!("------- running rsync -------");
    let r = Ok(Command::new("rsync").args(args).spawn()?.wait().await?);
    info!("-----------------------------");
    r
}

async fn route_to_ssh_hops(opts: &SshOpts, config: &'static Config) -> anyhow::Result<Vec<String>> {
    let statuses = fetch_statuses(config).await?;
    // TODO: there might be stale statuses here
    if statuses.is_empty() {
        debug!("there are no statuses");
    }

    debug!("statuses: {:?}", statuses);
    let graph = NetGraph::from_iter(
        statuses
            .iter()
            .inspect(|(n, _)| debug!("found machine: '{}'", n))
            .map(|(_, m)| m),
    );
    debug!("graph: {:?}", graph);

    let path = match graph.find_path(&get_hostname().await?, &opts.destination) {
        Some(path) => dbg!(path),
        None => {
            return Err(anyhow::anyhow!(
                "Path could not be found to '{}'",
                opts.destination
            ));
        }
    };
    debug!("path: {:?}", path);

    Ok(path_to_args(
        &path,
        opts.username.clone().unwrap_or_else(whoami::username),
    ))
}

async fn fetch_statuses(config: &'static Config) -> anyhow::Result<HashMap<String, MachineStatus>> {
    let client = AuthenticatedClient::new(config.token.clone(), &config.backend_url)?;
    let statuses = client
        .get("/machine/status")
        .expect("route shoud be well constructed")
        .send()
        .await?
        .json::<HashMap<String, MachineStatus>>()
        .await?;
    Ok(statuses)
}

fn path_to_args(path: &[IpAddr], username: String) -> Vec<String> {
    info!("{}@localhost -> {}", username, path.iter().format(" -> "));
    let mut args = vec![];
    let (path, tail) = path.split_at(path.len().saturating_sub(1));
    for ip in path {
        args.push(String::from("-t"));
        args.push(format!("{}@{}", username, ip));
        args.push(String::from("ssh"))
    }
    if let [last] = tail {
        args.push(format!("{}@{}", username, last));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        iter::repeat,
        net::{IpAddr, Ipv4Addr},
    };

    #[test]
    fn one_hop() {
        let expect = ["-t", "user@192.168.1.1", "ssh", "user@192.168.1.1"];
        let path = repeat(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(path_to_args(&path, "user".into()), expect);
    }

    #[test]
    fn no_hop() {
        let expect = ["user@192.168.1.1"];
        let path = repeat(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
            .take(1)
            .collect::<Vec<_>>();
        assert_eq!(path_to_args(&path, "user".into()), expect);
    }

    #[test]
    fn three_hops() {
        let expect = [
            "-t",
            "user@192.168.1.1",
            "ssh",
            "-t",
            "user@192.168.1.1",
            "ssh",
            "user@192.168.1.1",
        ];
        let path = repeat(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
            .take(3)
            .collect::<Vec<_>>();
        assert_eq!(path_to_args(&path, "user".into()), expect);
    }
}
