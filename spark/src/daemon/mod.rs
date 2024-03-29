//! Tasks are the background tasks that will be executed by the daemon

pub(crate) mod ipc;
pub(crate) mod machine_status;
pub(crate) mod persistent_conn;

use crate::config::Config;
use std::sync::Arc;

pub async fn run_all(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(?e, "failed to setup ctrl c handler");
            std::future::pending().await
        }
    };
    let persistent_conn = persistent_conn::start(config.clone());
    let ipc = ipc::start(config.clone()).await?;
    let machine_status = machine_status::start(config.clone())?;
    let background_tasks = async { tokio::join!(persistent_conn, ipc, machine_status) };

    tokio::select! {
        () = ctrl_c => {
            tracing::info!("shutting down");
        },
        _ = background_tasks => {
            tracing::warn!("all background tasks returned");
        }
    }
    Ok(())
}
