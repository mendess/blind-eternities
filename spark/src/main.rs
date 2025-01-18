mod backend;
mod config;
mod daemon;
mod routing;
mod util;

use std::{
    io::IsTerminal, os::unix::prelude::ExitStatusExt, path::PathBuf, process::ExitStatus,
    time::Duration,
};

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use common::{
    domain::Hostname,
    telemetry::{get_subscriber_no_bunny, init_subscriber},
};
use spark_protocol::{Command, ResponseExt};

/// A spark to travel the blind eternities!
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Enable verbose logging
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// run as a daemon
    Daemon,
    /// msg
    Msg {
        #[arg(long)]
        hostname: Option<Hostname>,
        #[command(subcommand)]
        msg: Command,
    },
    #[command(flatten)]
    SshInline(SshToolInline),
    /// ssh tooling
    #[command(subcommand)]
    Route(SshTool),
    /// remote music control
    Music {
        hostname: Hostname,
        #[command(subcommand)]
        cmd: spark_protocol::music::MusicCmdKind,
        #[arg(short, long, default_value_t = false)]
        session: bool,
    },
    /// Query the backend
    #[command(subcommand)]
    Backend(Backend),
    /// Print version
    Version,
    /// Generate Compleations
    AutoComplete { shell: clap_complete::Shell },
}

#[derive(Subcommand, Debug)]
enum SshToolInline {
    Ssh(routing::SshCommandOpts),
    Rsync(routing::RsyncOpts),
}

#[derive(Subcommand, Debug)]
enum SshTool {
    Ssh(routing::SshCommandOpts),
    Rsync(routing::RsyncOpts),
    CopyId(routing::SshOpts),
    Show(routing::ShowRouteOpts),
}

#[derive(Subcommand, Debug)]
enum Backend {
    /// list persistent connections
    Persistents,
    /// add a music auth token
    CreateMusicSession {
        #[arg(default_value_t = Hostname::from_this_host().unwrap())]
        hostname: Hostname,
        #[arg(short, long, value_parser = humantime::parse_duration)]
        expire_in: Option<Duration>,
        #[arg(short = 'l', long)]
        show_link: bool,
    },
    /// delete a music auth token
    DeleteMusicSession { session: String },
}

async fn app(args: Args) -> anyhow::Result<ExitStatus> {
    tracing::debug!("loading configuration");
    let config = config::load_configuration(args.config).context("loading configuration")?;

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
        Cmd::Msg { hostname, msg } => {
            let response = match hostname {
                None => daemon::ipc::send(&msg, config).await?,
                Some(hostname) => daemon::persistent_conn::send(config, hostname, msg).await?,
            };
            show_response(response);
            Ok(ExitStatus::from_raw(0))
        }
        Cmd::Music {
            session,
            cmd,
            hostname,
        } => {
            let response = if session {
                daemon::persistent_conn::send_to_session(config, hostname.into_string(), cmd)
                    .await?
            } else {
                daemon::persistent_conn::send(config, hostname, cmd.into()).await?
            };
            show_response(response);
            Ok(ExitStatus::from_raw(0))
        }
        Cmd::Backend(cmd) => backend::handle(cmd, config)
            .await
            .map(|_| ExitStatus::from_raw(0)),
        Cmd::Version => {
            tracing::warn!("version subcommand is deprecated and will be removed");
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(ExitStatus::from_raw(0))
        }
        Cmd::AutoComplete { shell } => {
            clap_complete::generate(shell, &mut Args::command(), "spark", &mut std::io::stdout());
            Ok(ExitStatus::from_raw(0))
        }
    }
}

fn show_response(response: spark_protocol::Response) {
    if std::io::stdout().is_terminal() {
        println!("{}", response.display());
    } else {
        serde_json::to_writer(std::io::stdout().lock(), &response).unwrap()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_subscriber(get_subscriber_no_bunny(
        if args.verbose { "debug" } else { "info" }.into(),
    ));

    let status = app(args).await?;
    std::process::exit(status.code().unwrap_or(139))
}
