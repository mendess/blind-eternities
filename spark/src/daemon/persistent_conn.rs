use std::{io, sync::Arc, time::Duration};

use common::{
    domain::Hostname,
    net::{
        MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, TalkJsonLinesExt, WriteJsonLinesExt,
    },
};
use tokio::{io::BufReader, net::TcpStream};

use crate::config::Config;

use super::ipc;

async fn run(config: &Config, hostname: &Hostname) -> io::Result<()> {
    let (read, mut write) =
        TcpStream::connect((config.backend_domain.as_str(), config.persistent_conn_port))
            .await?
            .into_split();
    let mut read = BufReader::new(read);
    let mut talker = (&mut read, &mut write);
    let syn = MetaProtocolSyn {
        hostname: hostname.clone(),
        token: config.token,
    };
    talker.talk::<_, MetaProtocolAck>(syn).await?;
    loop {
        let cmd = match read.recv::<spark_protocol::Local>().await? {
            Some(cmd) => cmd,
            None => return Ok(()),
        };

        let write = &mut write;
        let send_response =
            |response: spark_protocol::Response| async move { write.send(response).await };
        let result = match cmd {
            spark_protocol::Local::Reload => ipc::reload::reload(send_response).await,
            spark_protocol::Local::Music(m) => ipc::music::handle(m, send_response).await,
        };
        if let Err(e) = result {
            tracing::error!(?e)
        }
    }
}

pub(super) async fn start(config: Arc<Config>) {
    if config.enable_persistent_conn {
        tracing::info!("starting persistent connection");
        let hostname = Hostname::from_this_host();
        loop {
            if let Err(e) = run(&config, &hostname).await {
                tracing::error!(?e, "persistent connection dropped");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
