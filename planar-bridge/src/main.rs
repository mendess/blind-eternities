mod cache;
mod metrics;
mod music;

use std::io;

use axum::{middleware::from_fn, Router};
use common::{
    net::auth_client::Client,
    telemetry::{get_subscriber_no_bunny, init_subscriber},
};
use config::File;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use url::Url;

#[derive(Serialize, Deserialize)]
struct Config {
    log_level: Option<String>,
    port: u16,
    backend_url: Url,
}

fn load_config() -> Result<Config, config::ConfigError> {
    config::Config::builder()
        .add_source(File::with_name("bridgerc").required(true))
        .add_source(
            config::Environment::default()
                .prefix("PLANAR_BRIDEG_")
                .separator("__"),
        )
        .build()
        .and_then(config::Config::try_deserialize)
}

type Backend = common::net::auth_client::Client;

#[tokio::main]
async fn main() -> io::Result<()> {
    let config = load_config().map_err(io::Error::other)?;

    init_subscriber(get_subscriber_no_bunny(
        config.log_level.unwrap_or_else(|| "info".to_string()),
    ));

    let client = Client::new(config.backend_url).map_err(io::Error::other)?;

    let router = Router::new()
        .nest("/music", music::routes())
        .nest_service("/assets", ServeDir::new("planar-bridge/assets"))
        .layer(from_fn(common::telemetry::metrics::RequestMetrics::as_fn))
        .with_state(client);

    match common::telemetry::metrics::start_metrics_endpoint("planar_bridge").await {
        Ok(fut) => {
            tokio::spawn(fut);
        }
        Err(error) => {
            tracing::warn!(?error, "failed to start metrics endpoint");
        }
    }

    println!("running on http://localhost:{}/music", config.port);
    axum::serve(TcpListener::bind(("0.0.0.0", config.port)).await?, router).await
}
