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
    tracing::debug!("loading socket path");
    let path = socket_path().await?;
    tracing::debug!(?path);
    let _ = tokio::fs::remove_file(&path).await;
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
    let socket = UnixStream::connect(path)
        .await
        .context("connecting to socket")?;
    let (r, mut w) = socket.into_split();
    let mut msg =
        serde_json::to_string(cmd).with_context(|| format!("serializing cmd: {:?}", cmd))?;
    msg.push('\n');
    w.write_all(msg.as_bytes())
        .await
        .context("writing command")?;
    let mut s = String::new();
    BufReader::new(r).read_line(&mut s).await?;
    println!("daemon: {}", s.trim());
    Ok(())
}

async fn handle_client(client: tokio::net::UnixStream) -> io::Result<()> {
    let (r, mut w) = client.into_split();
    let mut reader = BufReader::new(r);
    let mut s = String::new();
    loop {
        match reader.read_line(&mut s).await {
            Ok(0) => break Ok(()),
            Err(e) => {
                tracing::error!("error reading line from client: {:?}", e)
            }
            _ => {}
        }

        let cmd = match serde_json::from_str::<Command>(&s) {
            Ok(cmd) => cmd,
            Err(e) => {
                w.write_all(e.to_string().as_bytes()).await?;
                w.write_all(b"\n").await?;
                continue;
            }
        };

        match cmd {
            Command::Reload => {
                let exe = match std::env::current_exe() {
                    Ok(exe) => exe,
                    Err(e) => {
                        w.write_all(e.to_string().as_bytes()).await?;
                        w.write_all(b"\n").await?;
                        continue;
                    }
                };
                tracing::info!("realoading spark daemon");
                w.write_all(b"about to exec, should be fine :)\n").await?;
                let e = std::process::Command::new(exe).arg("daemon").exec();
                w.write_all(e.to_string().as_bytes()).await?;
                w.write_all(b"\n").await?;
            }
        }
    }
}
