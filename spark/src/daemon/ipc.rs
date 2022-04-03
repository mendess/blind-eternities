mod music;

use std::{os::unix::prelude::CommandExt, sync::Arc};

use common::net::AuthenticatedClient;
use spark_protocol::{Backend, Command, ErrorResponse, Local, Remote, Response};
use structopt::StructOpt;

use crate::config::Config;

#[derive(StructOpt, Debug)]
pub enum SparkCommand {
    Reload,
}

/// From the cli, send a command to the local running daemon
pub async fn send(cmd: &SparkCommand) -> anyhow::Result<Result<Response, ErrorResponse>> {
    let r = match cmd {
        SparkCommand::Reload => spark_protocol::client::send(Command::Local(Local::Reload)).await,
    };
    Ok(r?)
}

pub async fn start(_config: Arc<Config>, client: Arc<AuthenticatedClient>) -> anyhow::Result<()> {
    spark_protocol::server::server(move |c| {
        let client = client.clone();
        async move {
            match c {
                Command::Local(l) => handle_local(l).await,
                Command::Remote(r) => handle_remote(r, &*client).await,
                Command::Backend(b) => handle_backend(b, &*client).await,
            }
        }
    })
    .await?;
    Ok(())
}

async fn handle_local(c: Local<'_>) -> Result<Response, ErrorResponse> {
    match c {
        Local::Reload => {
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(e) => return Err(ErrorResponse::RequestFailed(e.to_string())),
            };
            tracing::info!("realoading spark daemon");
            let e = std::process::Command::new(exe).arg("daemon").exec();
            Err(ErrorResponse::RequestFailed(e.to_string()))
        }
        Local::Music(m) => music::local(m).await,
    }
}

async fn handle_remote(
    r: Remote<'_>,
    client: &AuthenticatedClient,
) -> Result<Response, ErrorResponse> {
    client
        .post(&format!("remote-spark/{}", r.machine))
        .expect("correct url")
        .json(&r.command)
        .send()
        .await
        .map_err(|e| ErrorResponse::NetworkError(e.to_string()))?
        .json()
        .await
        .map_err(|e| ErrorResponse::DeserializingResponse(e.to_string()))
}

async fn handle_backend(
    b: Backend<'_>,
    client: &AuthenticatedClient,
) -> Result<Response, ErrorResponse> {
    match b {
        Backend::Music(meta) => music::backend(meta, client).await,
    }
}
