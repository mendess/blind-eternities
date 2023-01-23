use std::{
    collections::HashMap,
    iter,
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
    algorithms::net_graph::{NetGraph, SimpleNode},
    domain::{machine_status::MachineStatusFull, Hostname},
    net::AuthenticatedClient,
};
use itertools::Itertools;
use structopt::StructOpt;
use tokio::{fs::File, process::Command};
use tracing::{debug, info};

use crate::{
    config::Config,
    util::{destination::Destination, get_current_status},
};

#[derive(Debug, Clone, Copy)]
enum PseudoTty {
    None,
    Allocate,
}

#[derive(StructOpt, Debug)]
pub(super) struct SshOpts {
    destination: Destination,
    #[structopt(long = "dry-run")]
    dry_run: bool,
}

#[derive(StructOpt, Debug)]
pub(super) struct SshCommandOpts {
    #[structopt(flatten)]
    core: SshOpts,
    #[structopt(short("c"), long("shell"), conflicts_with("args"))]
    sub_shell: Option<String>,
    args: Vec<String>,
}

pub(super) async fn ssh(opts: &SshCommandOpts, config: &Config) -> anyhow::Result<ExitStatus> {
    let args = route_to_ssh_hops(&opts.core.destination, config, PseudoTty::Allocate)
        .await
        .context("getting ssh hops")?;
    let (ssh, args) = args
        .split_first()
        .expect("There should be at least one string here 🤔");

    let mut cmd = std::process::Command::new(ssh);
    cmd.args(args);
    match &opts.sub_shell {
        Some(script) => cmd.args(["bash", "-c", script]),
        None => cmd.args(&opts.args),
    };
    debug!("running ssh with args [{:?}]", cmd.get_args().format(", "));
    if opts.core.dry_run {
        Ok(ExitStatus::from_raw(0))
    } else {
        Ok(Command::from(cmd)
            .spawn()?
            .wait()
            .await
            .context("waiting for the ssh command")?)
    }
}

#[derive(StructOpt, Debug)]
pub(super) struct RsyncOpts {
    rsync_options: String,
    #[structopt(long = "dry-run")]
    dry_run: bool,
    paths: Vec<String>,
}

fn get_host<S: AsRef<str>>(
    paths: &[S],
) -> Option<Result<Destination, <Destination as FromStr>::Err>> {
    paths
        .iter()
        .find_map(|p| p.as_ref().split_once(':').map(|(host, _)| host))
        .map(Destination::from_str)
}

pub(super) async fn rsync(opts: &RsyncOpts, config: &Config) -> anyhow::Result<ExitStatus> {
    let host =
        get_host(&opts.paths).ok_or_else(|| anyhow::anyhow!("not remote host specified"))??;
    #[allow(unstable_name_collisions)]
    let bridge = route_to_ssh_hops(&host, config, PseudoTty::None)
        .await?
        .iter()
        .map(|s| s.as_str())
        .intersperse(" ")
        .collect::<String>();
    let mut cmd = std::process::Command::new("rsync");
    cmd.arg(format!(
        "-{}{}",
        opts.rsync_options,
        if opts.dry_run { "n" } else { "" }
    ));
    cmd.args(["-e", &bridge]);
    for f in &opts.paths {
        match f.split_once(':') {
            Some((_, path)) => cmd.arg(format!(":{path}")),
            None => cmd.arg(f),
        };
    }
    debug!(
        "running rsync with args: [{:?}]",
        cmd.get_args().format(", ")
    );
    info!("------- running rsync -------");
    let r = Ok(Command::from(cmd)
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
    #[structopt(short, long)]
    list: bool,
}

pub(super) async fn show_route(opts: &ShowRouteOpts, config: &Config) -> anyhow::Result<()> {
    let (statuses, hostname) = fetch_statuses(config).await?;

    if opts.list {
        for s in statuses.keys() {
            println!("{}", s);
        }
        return Ok(());
    }

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
    let (username, hostname) = opts.destination.resolve_alias(&config.network.aliases);

    let path = find_path(&opts.destination, config, hostname).await?;

    let args = path_to_args(&path, &username, PseudoTty::None);

    let mut cmd = Vec::new();

    let mut copy_id_cmd = String::from("ssh-copy-id");
    for partial_cmd in args {
        let program_index = cmd.len();
        partial_cmd.extend_args_with(|s| cmd.push(s), ["-o", "BatchMode=yes"]);
        cmd.push("true".into());
        tracing::debug!("running {:?}", cmd);
        let has_key_setup = Command::new(&cmd[0])
            .args(&cmd[1..])
            .status()
            .await?
            .success();
        cmd.pop(); // remove true command

        if !has_key_setup {
            let ssh = replace(&mut cmd[program_index], copy_id_cmd);
            tracing::debug!("running {:?}", cmd);
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
    destination: &Destination,
    config: &Config,
    dest_hostname: &Hostname,
) -> anyhow::Result<Vec<SimpleNode>> {
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
                "Path could not be found to '{destination}'",
            ));
        }
    };

    debug!(?path, "found a path");

    Ok(path)
}

async fn route_to_ssh_hops(
    destination: &Destination,
    config: &Config,
    pseudo_tty: PseudoTty,
) -> anyhow::Result<Vec<String>> {
    let (username, hostname) = destination.resolve_alias(&config.network.aliases);

    let path = find_path(destination, config, hostname).await?;

    Ok(path_to_args(&path, &username, pseudo_tty)
        .flatten()
        .collect())
}

async fn fetch_statuses(
    config: &Config,
) -> anyhow::Result<(HashMap<String, MachineStatusFull>, Hostname)> {
    let client = AuthenticatedClient::try_from(config)
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
    path: &'a [SimpleNode],
    username: &'a str,
    pseudo_tty: PseudoTty,
) -> impl Iterator<Item = SshCommand<'a>> {
    info!(
        "{}",
        iter::once((Some(username), IpAddr::V4(Ipv4Addr::LOCALHOST), 22))
            .chain(
                path.iter()
                    .map(|node| (node.default_username.as_deref(), node.ip, node.port))
            )
            .format_with(" -> ", |(def_user, ip, port), f| f(&format_args!(
                "{}@{}:{}",
                def_user.unwrap_or(username),
                ip,
                port
            )))
    );
    path.iter().map(
        move |SimpleNode {
                  default_username,
                  ip,
                  port,
              }| SshCommand {
            ip: *ip,
            port: *port,
            username: default_username.as_deref().unwrap_or(username),
            tty: pseudo_tty,
        },
    )
}

fn build_net_graph(statuses: &HashMap<String, MachineStatusFull>) -> NetGraph<'_> {
    NetGraph::from_iter(
        statuses
            .iter()
            .inspect(|(n, _)| debug!("found machine: '{}'", n))
            .map(|(_, m)| m),
    )
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
        let path = repeat(SimpleNode {
            default_username: None,
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            port: 222,
        })
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
        let path = repeat(SimpleNode {
            default_username: None,
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            port: 22,
        })
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
        let path = repeat(SimpleNode {
            default_username: None,
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            port: 22,
        })
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
        let path = repeat(SimpleNode {
            default_username: None,
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            port: 22,
        })
        .take(3)
        .collect::<Vec<_>>();
        assert_eq!(
            path_to_args(&path, "user", PseudoTty::None)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }

    #[test]
    fn correct_usernames_are_picked() {
        let expect = [
            "ssh",
            "-p",
            "22",
            "mendess@192.168.1.1",
            "ssh",
            "-p",
            "22",
            "pedromendes@192.168.1.1",
        ];
        let path = vec![
            SimpleNode {
                default_username: Some("mendess".into()),
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                port: 22,
            },
            SimpleNode {
                default_username: None,
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                port: 22,
            },
        ];
        assert_eq!(
            path_to_args(&path, "pedromendes", PseudoTty::None)
                .flatten()
                .collect::<Vec<_>>(),
            expect
        );
    }
}
