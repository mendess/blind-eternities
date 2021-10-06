mod config;
mod tasks;

use common::telemetry::{get_subscriber, init_subscriber};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_subscriber(get_subscriber(
        "spark".into(),
        "info".into(),
        std::io::stderr,
    ));
    let config = config::load_configuration().unwrap();

    tokio::spawn(tasks::machine_status::start(config)).await??;

    Ok(())
}
