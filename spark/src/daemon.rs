//! Tasks are the background tasks that will be executed by the daemon

use crate::config::Config;

pub(crate) mod machine_status;

pub async fn run_all(config: &'static Config) -> anyhow::Result<()> {
    tokio::spawn(machine_status::start(config)).await??;
    Ok(())
}
