//! Tasks are the background tasks that will be executed by the daemon

pub(crate) mod ipc;
pub(crate) mod machine_status;
pub(crate) mod persistent_conn;

use crate::config::Config;
use std::sync::Arc;

pub async fn run_all(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    tokio::try_join!(
        tokio::spawn(persistent_conn::start(config.clone())),
        tokio::spawn(ipc::start(config.clone()).await?),
        tokio::spawn(machine_status::start(config.clone())?),
    )?;
    Ok(())
}
