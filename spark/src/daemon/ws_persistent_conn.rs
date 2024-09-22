use std::{sync::Arc, time::Duration};

use anyhow::Context;
use common::{domain::Hostname, net::AuthenticatedClient, ws};
use futures::FutureExt;
use rust_socketio::{
    asynchronous::{Client, ClientBuilder},
    AckId, Payload,
};
use serde_json::json;
use spark_protocol::music::MusicCmdKind;

use crate::config::Config;

use super::handle_message;

async fn handler(payload: Payload, socket: Client, ack: AckId) {
    let [command]: [spark_protocol::Command; 1] = match payload {
        Payload::Text(values) => match serde_json::from_value(serde_json::Value::Array(values)) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = ?e, "invalid command sent from server");
                return;
            }
        },
        Payload::Binary(_) => panic!("unexpected bytes"),
        #[allow(deprecated)]
        Payload::String(_) => panic!("Payload::String panicked"),
    };

    tracing::info!(?command, "received command");
    let response = handle_message::rxtx(command).await;

    let e = socket
        .ack(ack, serde_json::to_string(&response).unwrap())
        .await;
    if let Err(e) = e {
        tracing::error!(error = ?e, "failed to send ack to server");
    }
}

async fn run(config: &Config, hostname: &Hostname, token: uuid::Uuid) -> anyhow::Result<()> {
    let socket = ClientBuilder::new(format!("{}?h={}", config.backend_domain, hostname))
        .auth(json! {{ "token": token.to_string() }})
        .namespace(ws::NS)
        .on_with_ack(ws::COMMAND, |payload, socket, ack| {
            handler(payload, socket, ack).boxed()
        })
        .on("error", |err, _| {
            async move { tracing::error!(error = ?err, "socket io error") }.boxed()
        })
        .connect()
        .await?;

    std::future::pending::<()>().await;
    socket.disconnect().await?;
    drop(socket);
    Ok(())
}

pub(super) async fn start(config: Arc<Config>) -> whoami::Result<()> {
    let hostname = Hostname::from_this_host()?;
    loop {
        tracing::info!("starting ws persistent connection");
        if let Err(e) = run(&config, &hostname, config.token).await {
            tracing::error!(?e, "persistent ws connection dropped");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub async fn send(
    config: Config,
    hostname: Hostname,
    command: spark_protocol::Command,
) -> anyhow::Result<spark_protocol::Response> {
    send_impl(
        AuthenticatedClient::try_from(&config)?
            .post(&format!("/persistent-connections/ws/send/{hostname}"))?
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
            .post(&format!("/music/ws/{}", session))?
            .json(&command),
    )
    .await
}

async fn send_impl(request: reqwest::RequestBuilder) -> anyhow::Result<spark_protocol::Response> {
    let resp = request
        .send()
        .await
        .context("sending request to ws/persistent-connections/send")?;
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
