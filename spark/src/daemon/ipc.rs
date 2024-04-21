use crate::config::Config;
use anyhow::Context;
use spark_protocol::{client::ClientBuilder, server::ServerBuilder, Command};
use std::{future::Future, io, sync::Arc};

use super::handle_message;

pub async fn start(config: Arc<Config>) -> io::Result<impl Future<Output = ()>> {
    ServerBuilder::new()
        .with_path(config.ipc_socket_path.clone())
        .serve(handle_message::rxtx)
        .await
}

pub async fn send(cmd: &Command, config: Config) -> anyhow::Result<spark_protocol::Response> {
    ClientBuilder::new()
        .with_path(config.ipc_socket_path)
        .build()
        .await
        .context("starting client")?
        .send(cmd)
        .await?
        .context("server shutdown")
}
