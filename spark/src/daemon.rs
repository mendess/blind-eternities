//! Tasks are the background tasks that will be executed by the daemon

use common::net::AuthenticatedClient;

use crate::config::Config;
use std::sync::Arc;

pub(crate) mod ipc;
pub(crate) mod machine_status;
pub(crate) mod music;

pub async fn run_all(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let client = Arc::new(AuthenticatedClient::new(
        config.token,
        &config.backend_domain,
        config.backend_port,
    )?);
    let _ = tokio::try_join!(
        tokio::spawn(ipc::remote_socket(config.clone(), client.clone())),
        tokio::spawn(ipc::start(config.clone(), client.clone())),
        tokio::spawn(machine_status::start(config.clone(), client.clone())),
        tokio::spawn(music::start(config.clone(), client.clone())),
    )?;
    Ok(())
}
