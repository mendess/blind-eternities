mod config;
mod daemon;
mod routing;

use std::{os::unix::prelude::ExitStatusExt, process::ExitStatus};

use common::telemetry::{get_subscriber_no_bunny, init_subscriber};
use routing::SshOpts;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Args {
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,
    #[structopt(subcommand)]
    cmd: Cmd,
}

#[derive(StructOpt, Debug)]
enum Cmd {
    /// run as a daemon
    Daemon,
    #[structopt(flatten)]
    Route(SshTool),
}

#[derive(StructOpt, Debug)]
enum SshTool {
    Ssh(SshOpts),
    Scp,
    Rsync,
}

async fn app() -> anyhow::Result<ExitStatus> {
    let args = Args::from_args();

    init_subscriber(get_subscriber_no_bunny(
        if args.verbose { "debug" } else { "info" }.into(),
    ));

    let config = config::load_configuration().unwrap();

    match args.cmd {
        Cmd::Daemon => daemon::run_all(config)
            .await
            .map(|_| ExitStatus::from_raw(1)),
        Cmd::Route(tool) => match tool {
            SshTool::Ssh(opts) => routing::ssh(opts, config).await,
            _ => todo!(),
        },
    }
}
#[tokio::main]
async fn main() {
    match app().await {
        Ok(status) => match status.code() {
            Some(c) => std::process::exit(c),
            None => std::process::exit(139),
        },
        Err(e) => {
            tracing::error!("{:#?}", e);
            std::process::exit(1)
        }
    }
}
