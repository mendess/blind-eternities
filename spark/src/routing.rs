use std::{collections::HashMap, net::IpAddr, process::ExitStatus};

use common::{
    algorithms::net_graph::NetGraph,
    domain::{Hostname, MachineStatus},
    net::AuthenticatedClient,
};
use structopt::StructOpt;
use tokio::process::Command;
use tracing::debug;

use crate::{config::Config, daemon::machine_status::get_hostname};

#[derive(StructOpt, Debug)]
pub(super) struct SshOpts {
    #[structopt(short = "-d", long = "--destination")]
    destination: Hostname,
    username: Option<String>,
}

pub(super) async fn ssh(opts: SshOpts, config: &'static Config) -> anyhow::Result<ExitStatus> {
    let client = AuthenticatedClient::new(config.token.clone(), &config.backend_url)?;
    let statuses = client
        .get("/machine/status")
        .expect("route shoud be well constructed")
        .send()
        .await?
        .json::<HashMap<String, MachineStatus>>()
        .await?;

    if statuses.is_empty() {
        debug!("there are no statuses");
    }

    let graph = NetGraph::from_iter(
        statuses
            .iter()
            .inspect(|(n, _)| debug!("{:#?}", n))
            .map(|(_, m)| m),
    );

    let path = match graph.find_path(&get_hostname().await?, &opts.destination) {
        Some(path) => path,
        None => {
            return Err(anyhow::anyhow!(
                "Path could not be found to '{}'",
                opts.destination
            ));
        }
    };

    let args = path_to_args(&path, opts.username);
    Ok(Command::new("ssh").args(args).spawn()?.wait().await?)
}

fn path_to_args(path: &[IpAddr], username: Option<String>) -> Vec<String> {
    let username = username.unwrap_or_else(whoami::username);
    // ssh [-t u@ip ssh -t u@ip2 ssh u@ip3]
    let mut args = vec![];
    let (path, last) = path.split_at(path.len().saturating_sub(1));
    for ip in path {
        args.push(String::from("-t"));
        args.push(format!("{}@{}", username, ip));
        args.push(String::from("ssh"))
    }
    if let [last] = last {
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
        assert_eq!(path_to_args(&path, Some("user".into())), expect);
    }

    #[test]
    fn no_hop() {
        let expect = ["user@192.168.1.1"];
        let path = repeat(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
            .take(1)
            .collect::<Vec<_>>();
        assert_eq!(path_to_args(&path, Some("user".into())), expect);
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
        assert_eq!(path_to_args(&path, Some("user".into())), expect);
    }
}
