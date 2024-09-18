use std::{sync::Arc, time::Duration};

use anyhow::{bail, Context};
use common::{
    domain::Hostname,
    net::{
        AuthenticatedClient, MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, TalkJsonLinesExt,
        WriteJsonLinesExt, PERSISTENT_CONN_RECV_TIMEOUT,
    },
};
use futures::FutureExt;
use spark_protocol::{music::MusicCmdKind, Command};
use tokio::{io::BufReader, net::TcpStream, time::timeout};

use crate::{
    config::{Config, PersistentConn},
    daemon::handle_message,
};

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
    async {
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
    }
    .await
    .context("SYN")?;
    tracing::info!("receiving commands");
    loop {
        let cmd = match timeout(
            PERSISTENT_CONN_RECV_TIMEOUT,
            read.recv::<spark_protocol::Command>(),
        )
        .await
        {
            Ok(r) => match r? {
                Some(cmd) => cmd,
                None => return Ok(()),
            },
            Err(_timeout) => bail!("receiving command timed out"),
        };
        if cmd != spark_protocol::Command::Heartbeat {
            tracing::info!(?cmd, "running command");
        }
        if let Err(e) = handle_message::rxtx(cmd).then(|msg| write.send(msg)).await {
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

pub async fn send(
    config: Config,
    hostname: Hostname,
    command: Command,
) -> anyhow::Result<spark_protocol::Response> {
    send_impl(
        AuthenticatedClient::try_from(&config)?
            .post(&format!("/persistent-connections/send/{hostname}"))?
            .json(&command),
    )
    .await
}

pub async fn send_to_session(
    config: Config,
    session: String,
    command: MusicCmdKind,
) -> anyhow::Result<spark_protocol::Response> {
    send_impl(
        AuthenticatedClient::try_from(&config)?
            .post(&format!("/music/{}", session))?
            .json(&command),
    )
    .await
}

async fn send_impl(request: reqwest::RequestBuilder) -> anyhow::Result<spark_protocol::Response> {
    let resp = request
        .send()
        .await
        .context("sending request to persistent-connections/send")?;
    if resp.status().is_success() {
        resp.json::<spark_protocol::Response>()
            .await
            .context("deserializing response")
    } else {
        let error = resp
            .error_for_status_ref()
            .expect_err("we checked that this wasn't successful");

        let text = resp
            .text()
            .await
            .context("failed to get body of request when processing remote error")
            .with_context(|| error.to_string())?;

        Err(anyhow::anyhow!("{text}")).context(error)
    }
}
