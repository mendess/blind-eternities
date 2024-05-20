mod assets;
mod cache;
mod metrics;
mod music;

use std::io;

use actix_web::{web::Data, App, HttpServer};
use common::{
    net::auth_client::Client,
    telemetry::{get_subscriber_no_bunny, init_subscriber},
};
use config::File;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize)]
struct Config {
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

#[actix_web::main]
async fn main() -> io::Result<()> {
    init_subscriber(get_subscriber_no_bunny("info".to_string()));

    let config = load_config().map_err(io::Error::other)?;
    let client = Data::new(Client::new(config.backend_url).map_err(io::Error::other)?);

    let server = HttpServer::new(move || {
        App::new()
            .wrap(common::telemetry::metrics::RequestMetrics)
            .service(music::routes())
            .service(assets::routes())
            .app_data(client.clone())
    });

    tokio::spawn(common::telemetry::metrics::start_metrics_endpoint(
        "planar_bridge",
    )?);

    println!("running on http://localhost:{}/music", config.port);
    server.bind(("0.0.0.0", config.port))?.run().await
}
