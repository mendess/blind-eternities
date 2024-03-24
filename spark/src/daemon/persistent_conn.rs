use std::{sync::Arc, time::Duration};

use anyhow::{bail, Context};
use common::{
    domain::Hostname,
    net::{
        MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, TalkJsonLinesExt, WriteJsonLinesExt,
        PERSISTENT_CONN_RECV_TIMEOUT,
    },
};
use spark_protocol::{ErrorResponse, SuccessfulResponse};
use tokio::{io::BufReader, net::TcpStream, time::timeout};

use crate::config::{Config, PersistentConn};

use super::ipc;

async fn run(config: &Config, hostname: &Hostname, port: u16) -> anyhow::Result<()> {
    let (read, mut write) = TcpStream::connect((
        config
            .backend_domain
            .host_str()
            .context("no host in backend domain")?,
        port,
    ))
    .await?
    .into_split();
    let mut read = BufReader::new(read);
    let mut talker = (&mut read, &mut write);
    let syn = MetaProtocolSyn {
        hostname: hostname.clone(),
        token: config.token,
    };
    tracing::info!(?syn, "starting protocol");
    let syn = || async {
        match talker.talk::<_, MetaProtocolAck>(syn).await? {
            None => bail!("server closed the connection without responding"),
            Some(MetaProtocolAck::Ok) => Ok(()),
            Some(MetaProtocolAck::BadToken(token)) => bail!("invalid token {token}"),
            Some(MetaProtocolAck::InvalidValue(value)) => bail!("invalid value {value}"),
            Some(MetaProtocolAck::DeserializationError {
                expected_type,
                error,
            }) => bail!("serialization error. Expected {expected_type}. Error: {error}"),
        }
    };
    syn().await.context("SYN")?;
    loop {
        tracing::info!("receiving command");
        let cmd = match timeout(
            PERSISTENT_CONN_RECV_TIMEOUT,
            read.recv::<spark_protocol::Local>(),
        )
        .await
        {
            Ok(r) => match r? {
                Some(cmd) => cmd,
                None => return Ok(()),
            },
            Err(_timeout) => bail!("receiving command timed out"),
        };
        if cmd != spark_protocol::Local::Heartbeat {
            tracing::info!(?cmd, "running command");
        }
        let write = &mut write;
        let send_response =
            |response: spark_protocol::Response| async move { write.send(response).await };
        let result = match cmd {
            spark_protocol::Local::Heartbeat => {
                send_response(Ok::<_, ErrorResponse>(SuccessfulResponse::Unit)).await
            }
            spark_protocol::Local::Reload => ipc::reload::reload(send_response).await,
            #[cfg(feature = "music-ctl")]
            spark_protocol::Local::Music(m) => ipc::music::handle(m, send_response).await,
            #[cfg(not(feature = "music-ctl"))]
            spark_protocol::Local::Music(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "music control is disabled on this machine",
            )),
        };
        if let Err(e) = result {
            tracing::error!(?e)
        }
    }
}

pub(super) async fn start(config: Arc<Config>) -> whoami::Result<()> {
    if let Some(PersistentConn { port }) = config.persistent_conn {
        let hostname = Hostname::from_this_host()?;
        loop {
            tracing::info!("starting persistent connection");
            if let Err(e) = run(&config, &hostname, port).await {
                tracing::error!(?e, "persistent connection dropped");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Ok(())
}
