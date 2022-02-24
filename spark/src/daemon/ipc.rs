use crate::config::Config;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{future::Future, io, os::unix::prelude::CommandExt, path::PathBuf, sync::Arc};
use structopt::StructOpt;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

#[derive(Clone, Debug, Deserialize, StructOpt, Serialize)]
pub enum Command {
    Reload,
}

async fn socket_path() -> io::Result<PathBuf> {
    let (path, e) = namespaced_tmp::async_impl::in_user_tmp("spark-socket").await;
    if let Some(e) = e {
        Err(e)
    } else {
        Ok(path)
    }
}

pub async fn start(_config: Arc<Config>) -> io::Result<impl Future<Output = ()>> {
    let path = socket_path().await?;
    tokio::fs::remove_file(&path).await?;
    tracing::info!("binding ipc socket: {:?}", path);
    let socket = UnixListener::bind(path)?;
    Ok(async move {
        loop {
            let (client, _) = match socket.accept().await {
                Ok(client) => client,
                Err(e) => {
                    tracing::error!("failed to accept a connection: {:?}", e);
                    break;
                }
            };
            tokio::spawn(handle_client(client));
        }
    })
}

pub async fn send(cmd: &Command) -> anyhow::Result<()> {
    let path = socket_path().await.context("getting socket path")?;
    let mut socket = UnixStream::connect(path)
        .await
        .context("connecting to socket")?;
    let mut msg =
        serde_json::to_string(cmd).with_context(|| format!("serializing cmd: {:?}", cmd))?;
    msg.push('\n');
    socket
        .write_all(msg.as_bytes())
        .await
        .context("writing command")?;
    Ok(())
}

async fn handle_client(client: tokio::net::UnixStream) {
    let (r, _w) = client.into_split();
    let mut reader = BufReader::new(r);
    let mut s = String::new();
    loop {
        match reader.read_line(&mut s).await {
            Ok(0) => break,
            Err(e) => {
                tracing::error!("error reading line from client: {:?}", e)
            }
            _ => {}
        }

        let cmd = match serde_json::from_str::<Command>(&s) {
            Ok(cmd) => cmd,
            Err(e) => {
                todo!("{:?}", e)
            }
        };

        match cmd {
            Command::Reload => {
                let exe = match std::env::current_exe() {
                    Ok(exe) => exe,
                    Err(e) => {
                        todo!("{:?}", e)
                    }
                };
                tracing::info!("realoading spark daemon");
                let e = std::process::Command::new(exe).arg("daemon").exec();
                todo!("{:?}", e)
            }
        }
    }
}
