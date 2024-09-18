//! Tasks are the background tasks that will be executed by the daemon

mod handle_message;
pub(crate) mod ipc;
pub(crate) mod machine_status;
#[cfg(not(feature = "ws"))]
pub(crate) mod persistent_conn;
#[cfg(feature = "ws")]
pub(crate) mod ws_persistent_conn;
#[cfg(feature = "ws")]
pub(crate) use ws_persistent_conn as persistent_conn;

use futures::future;

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
    #[cfg(feature = "ws")]
    let persistent_conn = persistent_conn::start(config.clone());
    #[cfg(not(feature = "ws"))]
    let persistent_conn = persistent_conn::start(config.clone());
    let ipc = ipc::start(config.clone()).await?;
    let machine_status = machine_status::start(config.clone())?;
    let background_tasks = future::join3(persistent_conn, ipc, machine_status);

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
