mod backend;
mod config;
mod daemon;
mod music;
mod routing;
mod util;

use std::{os::unix::prelude::ExitStatusExt, process::ExitStatus};

use anyhow::Context;
use common::telemetry::{get_subscriber_no_bunny, init_subscriber};
use daemon::ipc::Command;
use structopt::StructOpt;
use util::destination::Destination;

/// A spark to travel the blind eternities!
#[derive(StructOpt, Debug)]
struct Args {
    /// Enable verbose logging
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,
    #[structopt(subcommand)]
    cmd: Cmd,
}

#[derive(StructOpt, Debug)]
enum Cmd {
    /// run as a daemon
    Daemon,
    /// msg
    Msg(Command),
    #[structopt(flatten)]
    SshInline(SshToolInline),
    /// ssh tooling
    Route(SshTool),
    /// remote music control
    Music {
        destination: Destination,
        #[structopt(flatten)]
        cmd: spark_protocol::music::MusicCmd,
    },
    /// Query the backend
    Backend(Backend),
}

#[derive(StructOpt, Debug)]
enum SshToolInline {
    Ssh(routing::SshCommandOpts),
    Rsync(routing::RsyncOpts),
}

#[derive(StructOpt, Debug)]
enum SshTool {
    Ssh(routing::SshCommandOpts),
    Rsync(routing::RsyncOpts),
    CopyId(routing::SshOpts),
    Show(routing::ShowRouteOpts),
}

#[derive(StructOpt, Debug)]
enum Backend {
    /// list persistent connections
    Persistents,
}

async fn app(args: Args) -> anyhow::Result<ExitStatus> {
    tracing::debug!("loading configuration");
    let config = config::load_configuration().context("loading configuration")?;

    tracing::debug!(?args.cmd);

    match args.cmd {
        Cmd::Daemon => daemon::run_all(config)
            .await
            .map(|_| ExitStatus::from_raw(1)),
        Cmd::Route(SshTool::Ssh(opts)) | Cmd::SshInline(SshToolInline::Ssh(opts)) => {
            routing::ssh(&opts, &config).await
        }
        Cmd::Route(SshTool::Rsync(opts)) | Cmd::SshInline(SshToolInline::Rsync(opts)) => {
            routing::rsync(&opts, &config).await
        }
        Cmd::Route(SshTool::Show(opts)) => routing::show_route(&opts, &config)
            .await
            .map(|_| ExitStatus::from_raw(0)),
        Cmd::Route(SshTool::CopyId(opts)) => routing::copy_id(&opts, &config).await,
        Cmd::Msg(msg) => daemon::ipc::send(&msg, config)
            .await
            .map(|_| ExitStatus::from_raw(0)),
        Cmd::Music { destination, cmd } => music::handle(destination, cmd, config)
            .await
            .map(|_| ExitStatus::from_raw(0)),
        Cmd::Backend(cmd) => backend::handle(cmd, config)
            .await
            .map(|_| ExitStatus::from_raw(0)),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::from_args();
    init_subscriber(get_subscriber_no_bunny(
        if args.verbose { "debug" } else { "info" }.into(),
    ));

    let status = app(args).await?;
    std::process::exit(status.code().unwrap_or(139))
}
