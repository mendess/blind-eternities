mod music;

use std::{os::unix::prelude::CommandExt, sync::Arc};

use common::{
    domain::Hostname,
    net::{AuthenticatedClient, MetaProtocolAck, ReadJsonLinesExt, WriteJsonLinesExt},
};
// use dashmap::DashMap;
// use once_cell::sync::Lazy;
use spark_protocol::{Backend, Command, Local, ProtocolError, ProtocolMsg, Remote};
use structopt::StructOpt;
use tokio::{
    io::{BufReader, BufWriter},
    net::TcpStream,
};

use crate::config::Config;

#[derive(StructOpt, Debug)]
pub enum SparkCommand {
    Reload,
}

/// From the cli, send a command to the local running daemon
pub async fn send(cmd: &SparkCommand) -> anyhow::Result<Result<ProtocolMsg, ProtocolError>> {
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
                Command::Remote(r) => handle_remote(r, &client).await,
                Command::Backend(b) => handle_backend(b, &*client).await,
            }
        }
    })
    .await?;
    Ok(())
}

pub async fn remote_socket(
    config: Arc<Config>,
    client: Arc<AuthenticatedClient>,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = {
        let conn =
            TcpStream::connect((config.backend_domain.as_str(), config.persistent_conn_port))
                .await?;
        let (r, w) = conn.into_split();
        (BufReader::new(r), BufWriter::new(w))
    };
    async fn handle_ack(r: MetaProtocolAck, what: &str) -> anyhow::Result<()> {
        if r != MetaProtocolAck::Ok {
            tracing::error!(ack = ?r, "sending {what}");
            Err(anyhow::anyhow!("error when sending {what}: {r:?}"))
        } else {
            Ok(())
        }
    }
    writer.send(Hostname::from_this_host()).await?;
    handle_ack(reader.recv().await?, "hostname").await?;
    writer.send(&client.token()).await?;
    handle_ack(reader.recv().await?, "token").await?;

    loop {
        tracing::info!("listening to commands");
        let local = reader.recv::<Local<'static>>().await;
        let response = match local {
            Err(e) => {
                tracing::error!(?e);
                continue;
            }
            Ok(local) => {
                tracing::info!(request = ?local);
                handle_local(local).await
            }
        };
        if let Err(e) = writer.send(response).await {
            tracing::error!(?e, "responding to request");
        }
    }
}

async fn handle_local(c: Local<'_>) -> Result<ProtocolMsg, ProtocolError> {
    match c {
        Local::Reload => {
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(e) => return Err(ProtocolError::RequestFailed(e.to_string())),
            };
            tracing::info!("realoading spark daemon");
            let e = std::process::Command::new(exe).arg("daemon").exec();
            Err(ProtocolError::RequestFailed(e.to_string()))
        }
        Local::Music(m) => music::local(m).await,
    }
}

type RemoteResponse = Result<ProtocolMsg, ProtocolError>;
// type Request = String;

// static CACHE: Lazy<DashMap<Request, RemoteResponse>> = Lazy::new(Default::default);

async fn handle_remote(
    r: Remote<'static>,
    client: &Arc<AuthenticatedClient>,
) -> Result<ProtocolMsg, ProtocolError> {
    // let request_key = format!("{:?}", r);
    async fn request(client: &AuthenticatedClient, r: &Remote<'_>) -> RemoteResponse {
        client
            .post(&format!("remote-spark/{}", r.machine))
            .expect("correct url")
            .json(&r.command)
            .send()
            .await
            .map_err(|e| ProtocolError::NetworkError(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProtocolError::DeserializingResponse(e.to_string()))
    }
    // match CACHE.get(&request_key) {
    //     Some(response) => {
    //         tokio::spawn({
    //             let client = client.clone();
    //             async move {
    //                 let response = request(&*client, &r).await;
    //                 CACHE.insert(request_key, response);
    //             }
    //         });
    //         response.clone()
    //     }
    //     None => {
    //         let response = request(&*client, &r).await;
    //         CACHE.insert(request_key, response.clone());
    //         response
    //     }
    // }
    request(&*client, &r).await
}

async fn handle_backend(
    b: Backend<'_>,
    client: &AuthenticatedClient,
) -> Result<ProtocolMsg, ProtocolError> {
    match b {
        Backend::Music(meta) => music::backend(meta, client).await,
    }
}
