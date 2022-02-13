use std::{
    collections::HashMap,
    fmt, iter,
    mem::replace,
    net::{IpAddr, Ipv4Addr},
    os::unix::prelude::ExitStatusExt,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    str::FromStr,
};

use anyhow::Context;
use arrayvec::ArrayVec;
use chrono::Utc;
use common::{
    algorithms::net_graph::NetGraph,
    domain::{
        machine_status::{MachineStatusFull, Port},
        Hostname,
    },
    net::AuthenticatedClient,
};
use itertools::Itertools;
use structopt::StructOpt;
use tokio::{fs::File, process::Command};
use tracing::{debug, info};

use crate::{config::Config, util::get_current_status};

#[derive(Debug, Clone, Copy)]
enum PseudoTty {
    None,
    Allocate,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DestinationRef {
    username: Option<String>,
    hostname: Hostname,
}

impl FromStr for DestinationRef {
    type Err = <Hostname as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('@') {
            Some((username, hostname)) => Ok(DestinationRef {
                hostname: hostname.parse()?,
                username: Some(username.parse::<Hostname>()?.into_string()),
            }),
            None => Ok(DestinationRef {
                hostname: s.parse()?,
                username: None,
            }),
        }
    }
}

impl fmt::Display for DestinationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(u) => write!(f, "{}@{}", u, self.hostname),
            None => write!(f, "{}", self.hostname),
        }
    }
}

#[derive(StructOpt, Debug)]
pub(super) struct SshOpts {
    destination: DestinationRef,
    #[structopt(long = "dry-run")]
    dry_run: bool,
}

pub(super) async fn ssh(opts: &SshOpts, config: &Config) -> anyhow::Result<ExitStatus> {
    let args = route_to_ssh_hops(opts, config, PseudoTty::Allocate)
        .await
        .context("getting ssh hops")?;
    let (ssh, args) = args
        .split_first()
        .expect("There should be at least one string here 🤔");

    debug!("running ssh with args {:?}", args);
    if opts.dry_run {
        Ok(ExitStatus::from_raw(0))
    } else {
        Ok(Command::new(ssh)
            .args(args)
            .spawn()?
            .wait()
            .await
            .context("waiting for the ssh command")?)
    }
}

#[derive(StructOpt, Debug)]
pub(super) struct RsyncOpts {
    rsync_options: String,
    #[structopt(flatten)]
    ssh_opts: SshOpts,
    paths: Vec<PathBuf>,
}

pub(super) async fn rsync(opts: &RsyncOpts, config: &Config) -> anyhow::Result<ExitStatus> {
    #[allow(unstable_name_collisions)]
    let bridge = route_to_ssh_hops(&opts.ssh_opts, config, PseudoTty::None)
        .await?
        .iter()
        .map(|s| s.as_str())
        .intersperse(" ")
        .collect();
    let mut args = vec![
        format!(
            "-{}{}",
            opts.rsync_options,
            if opts.ssh_opts.dry_run { "n" } else { "" }
        ),
        "-e".into(),
        bridge,
    ];
    args.reserve(opts.paths.len());
    let (files, dest) = opts.paths.split_at(opts.paths.len().saturating_sub(1));
    for f in files {
        args.push(f.to_str().unwrap().to_string());
    }
    if let [dest] = dest {
        args.push(format!(":{}", dest.to_str().unwrap()))
    }
    debug!("running rsync with args: {:?}", args);
    info!("------- running rsync -------");
    let r = Ok(Command::new("rsync")
        .args(args)
        .spawn()?
        .wait()
        .await
        .context("waiting for rsync")?);
    info!("-----------------------------");
    r
}

#[derive(Debug, StructOpt)]
pub struct ShowRouteOpts {
    #[structopt(short, long)]
    filename: Option<PathBuf>,
    #[structopt(short, long)]
    destination: Option<Hostname>,
}

pub(super) async fn show_route(opts: &ShowRouteOpts, config: &Config) -> anyhow::Result<()> {
    let (statuses, hostname) = fetch_statuses(config).await?;

    let graph = build_net_graph(&statuses);

    let path = match opts.destination.as_ref() {
        Some(d) => graph.find_path(&hostname, d),
        None => None,
    };
    match &opts.filename {
        Some(filename) => {
            let file = File::create(filename).await.context("creating dot file")?;
            graph
                .to_dot(file, path.as_deref())
                .await
                .context("writing dot file")?;
        }
        None => {
            let (file, temp_path) = tempfile::NamedTempFile::new()?.into_parts();
            let mut dot = Command::new("dot")
                .arg("-Tpng")
                .stdin(Stdio::piped())
                .stdout(file)
                .spawn()
                .context("rendering dot to png")?;
            graph
                .to_dot(
                    dot.stdin
                        .take()
                        .ok_or_else(|| anyhow::anyhow!("can't get stdin of dot"))?,
                    path.as_deref(),
                )
                .await?;
            let status = dot
                .wait()
                .await
                .context("waiting for dot to png conversion")?;
            if status.success() {
                open::that(&temp_path)
                    .with_context(|| format!("opening rendered graph: {}", temp_path.display()))?;
            } else {
                return Err(anyhow::anyhow!("dot finished with exit code: {}", status));
            }
        }
    }
    Ok(())
}

pub(crate) async fn copy_id(opts: &SshOpts, config: &Config) -> anyhow::Result<ExitStatus> {
    let (username, hostname) = resolve_alias(&config.network.aliases, &opts.destination);

    let path = find_path(opts, config, hostname).await?;

    let username = username.cloned().unwrap_or_else(whoami::username);

    let args = path_to_args(&path, &username, PseudoTty::None);

    let mut cmd = Vec::<String>::new();

    let mut copy_id_cmd = String::from("ssh-copy-id");
    for partial_cmd in args {
        let program_index = cmd.len();
        partial_cmd.extend_args_with(|s| cmd.push(s), ["-o", "BatchMode=yes"]);
        cmd.push("true".into());
        tracing::debug!("running {:?}", &cmd[program_index..]);
        let has_key_setup = Command::new(&cmd[program_index])
            .args(&cmd[(program_index + 1)..])
            .status()
            .await?
            .success();
        cmd.pop(); // remove true command

        if !has_key_setup {
            let ssh = replace(&mut cmd[program_index], copy_id_cmd);
            tracing::debug!(
                "running {:?}",
                &cmd[program_index..(cmd.len().saturating_sub(2))]
            );
            if !opts.dry_run {
                let status = Command::new(&cmd[0])
                    .args(&cmd[1..(cmd.len().saturating_sub(2))])
                    .spawn()?
                    .wait()
                    .await?;
                if !status.success() {
                    return Err(anyhow::anyhow!("failed to run copy id"));
                }
            }
            copy_id_cmd = replace(&mut cmd[program_index], ssh);
        }
    }

    Ok(ExitStatus::from_raw(0))
}

async fn find_path(
    opts: &SshOpts,
    config: &Config,
    dest_hostname: &Hostname,
) -> anyhow::Result<Vec<(IpAddr, u16)>> {
    let (statuses, hostname) = fetch_statuses(config).await?;
    // TODO: there might be stale statuses here
    if statuses.is_empty() {
        debug!("there are no statuses");
    }

    let graph = build_net_graph(&statuses);

    let path = match graph
        .find_path(&hostname, dest_hostname)
        .and_then(|p| graph.path_to_ips(&p))
    {
        Some(mut path) => {
            // if we have more than one target we can skip localhost
            if path.len() > 1 {
                path.remove(0);
            }
            path
        }
        None => {
            return Err(anyhow::anyhow!(
                "Path could not be found to '{}'",
                opts.destination
            ));
        }
    };

    Ok(path)
}

async fn route_to_ssh_hops(
    opts: &SshOpts,
    config: &Config,
    pseudo_tty: PseudoTty,
) -> anyhow::Result<Vec<String>> {
    let (username, hostname) = resolve_alias(&config.network.aliases, &opts.destination);

    let path = find_path(opts, config, hostname).await?;

    Ok(match username {
        Some(u) => path_to_args(&path, u, pseudo_tty).flatten().collect(),
        None => path_to_args(&path, &whoami::username(), pseudo_tty)
            .flatten()
            .collect(),
    })
}

async fn fetch_statuses(
    config: &Config,
) -> anyhow::Result<(HashMap<String, MachineStatusFull>, Hostname)> {
    let client = AuthenticatedClient::new(config.token.clone(), &config.backend_url)
        .context("creating an authenticated client")?;
    let response = client
        .get("/machine/status")
        .expect("route should be well constructed")
        .send()
        .await
        .context("requesting statuses from backend")?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "http error: {}",
            response.status().canonical_reason().unwrap_or("unknown")
        ));
    }
    let mut statuses = response
        .json::<HashMap<String, MachineStatusFull>>()
        .await
        .context("parsing status json")?;

    let this = MachineStatusFull {
        fields: get_current_status(config)
            .await
            .context("getting the local status")?,
        last_heartbeat: Utc::now().naive_utc(),
    };

    let hostname = this.hostname.clone();
    statuses.insert(this.hostname.to_string(), this);

    Ok((statuses, hostname))
}

struct SshCommand<'u> {
    port: u16,
    tty: PseudoTty,
    username: &'u str,
    ip: IpAddr,
}

impl SshCommand<'_> {
    fn extend_args_with<F, E, I>(&self, mut push: F, extra_args: E)
    where
        F: FnMut(String),
        E: IntoIterator<Item = I>,
        I: Into<String>,
    {
        push("ssh".into());
        push("-p".into());
        push(self.port.to_string());
        if let PseudoTty::Allocate = self.tty {
            push("-t".into());
        }
        push(format!("{}@{}", self.username, self.ip));
        for a in extra_args {
            push(a.into())
        }
    }

    fn extend_args<F: FnMut(String)>(&self, push: F) {
        self.extend_args_with::<_, _, String>(push, [])
    }
}

impl IntoIterator for SshCommand<'_> {
    type IntoIter = <ArrayVec<String, 5> as IntoIterator>::IntoIter;
    type Item = <ArrayVec<String, 5> as IntoIterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        let mut args = ArrayVec::new();
        self.extend_args(|s| args.push(s));
        args.into_iter()
    }
}

fn path_to_args<'a>(
    path: &'a [(IpAddr, Port)],
    username: &'a str,
    pseudo_tty: PseudoTty,
) -> impl Iterator<Item = SshCommand<'a>> {
    info!(
        "user: {} => {}",
        username,
        iter::once(&(IpAddr::V4(Ipv4Addr::LOCALHOST), 22))
            .chain(path.iter())
            .format_with(" -> ", |(ip, port), f| f(&format_args!("{}:{}", ip, port)))
    );
    path.iter().map(move |&(ip, port)| SshCommand {
        ip,
        port,
        username,
        tty: pseudo_tty,
    })
}

fn build_net_graph(statuses: &HashMap<String, MachineStatusFull>) -> NetGraph<'_> {
    NetGraph::from_iter(
        statuses
            .iter()
            .inspect(|(n, _)| debug!("found machine: '{}'", n))
            .map(|(_, m)| m),
    )
}

fn resolve_alias<'a>(
    aliases: &'a HashMap<String, DestinationRef>,
    dest: &'a DestinationRef,
) -> (Option<&'a String>, &'a Hostname) {
    match aliases.get(dest.hostname.as_ref()) {
        Some(d) => {
            tracing::debug!("resolving alias {} as {}", dest.hostname, d);
            (
                dest.username.as_ref().or_else(|| d.username.as_ref()),
                &d.hostname,
            )
        }
        None => (dest.username.as_ref(), &dest.hostname),
    }
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
        let expect = [
            "ssh",
            "-p",
            "222",
            "-t",
            "user@192.168.1.1",
            "ssh",
            "-p",
            "222",
            "-t",
            "user@192.168.1.1",
        ];
        let path = repeat((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 222))
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(
            path_to_args(&path, "user", PseudoTty::Allocate)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }

    #[test]
    fn no_hop() {
        let expect = ["ssh", "-p", "22", "-t", "user@192.168.1.1"];
        let path = repeat((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 22))
            .take(1)
            .collect::<Vec<_>>();
        assert_eq!(
            path_to_args(&path, "user", PseudoTty::Allocate)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }

    #[test]
    fn three_hops() {
        let expect = [
            "ssh",
            "-p",
            "22",
            "-t",
            "user@192.168.1.1",
            "ssh",
            "-p",
            "22",
            "-t",
            "user@192.168.1.1",
            "ssh",
            "-p",
            "22",
            "-t",
            "user@192.168.1.1",
        ];
        let path = repeat((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 22))
            .take(3)
            .collect::<Vec<_>>();
        assert_eq!(
            path_to_args(&path, "user", PseudoTty::Allocate)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }

    #[test]
    fn three_hops_no_tty() {
        let expect = [
            "ssh",
            "-p",
            "22",
            "user@192.168.1.1",
            "ssh",
            "-p",
            "22",
            "user@192.168.1.1",
            "ssh",
            "-p",
            "22",
            "user@192.168.1.1",
        ];
        let path = repeat((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 22))
            .take(3)
            .collect::<Vec<_>>();
        assert_eq!(
            path_to_args(&path, "user", PseudoTty::None)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }
}
