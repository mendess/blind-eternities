mod cache;
mod files;
mod games;
mod metrics;
mod music;
mod playlist;
mod util;
mod walls;

use std::io;

use askama::Template;
use axum::{
    Router,
    response::{Html, IntoResponse},
};
use clap::Parser;
use common::{
    net::auth_client::Client,
    telemetry::{get_subscriber_no_bunny, init_subscriber, metrics::MetricsEndpoint},
};
use config::File;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("mlib error: {0}")]
    Mlib(#[from] mlib::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    #[error("render error: {0}")]
    TemplateRender(#[from] askama::Error),
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) | Self::Reqwest(_) | Self::Mlib(_) | Self::TemplateRender(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    log_level: Option<String>,
    port: u16,
    backend_url: Url,
    #[serde(default = "default_metrics_port")]
    metrics_port: u16,
    games: games::Config,
}

fn default_metrics_port() -> u16 {
    9000
}

fn load_config(path: Option<&str>) -> Result<Config, config::ConfigError> {
    config::Config::builder()
        .add_source(File::with_name(path.unwrap_or("bridgerc")).required(true))
        .add_source(
            config::Environment::default()
                .prefix("PLANAR_BRIDEG_")
                .separator("__"),
        )
        .build()
        .and_then(config::Config::try_deserialize)
}

type Backend = common::net::auth_client::Client;

#[derive(clap::Parser)]
struct Args {
    #[arg(short, long)]
    config: Option<String>,
}

#[derive(Clone)]
struct RouterState {
    client: Client,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let Args { config } = Args::parse();
    let config = load_config(config.as_deref()).map_err(io::Error::other)?;

    init_subscriber(get_subscriber_no_bunny(
        config.log_level.as_deref().unwrap_or("info"),
    ));

    let client = Client::new(config.backend_url.clone()).map_err(io::Error::other)?;

    let MetricsEndpoint { worker, layer } = common::telemetry::metrics::start_metrics_endpoint(
        "planar_bridge",
        TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, config.metrics_port)).await?,
    );
    let state = RouterState { client };
    tokio::spawn(worker);
    let router = Router::new()
        .route("/", axum::routing::get(index))
        .nest("/music", music::routes())
        .nest("/playlist", playlist::routes())
        .merge(util::append_slash_router(&["/walls"]))
        .nest("/walls/", walls::routes())
        .nest("/files", files::routes())
        .nest("/games", games::router(config.games))
        .nest_service(
            "/favicon.ico",
            ServeFile::new("planar-bridge/assets/icons/favicon.ico"),
        )
        .nest_service("/assets", ServeDir::new("planar-bridge/assets"))
        .fallback(not_found)
        .layer(layer)
        .with_state(state);

    println!("running on http://localhost:{}", config.port);
    axum::serve(TcpListener::bind(("0.0.0.0", config.port)).await?, router).await
}

async fn index() -> impl IntoResponse {
    #[derive(Template)]
    #[template(path = "index.html")]
    struct Index;

    Html(Index.render().unwrap())
}

async fn not_found() -> impl IntoResponse {
    #[derive(Template)]
    #[template(path = "not-found.html")]
    struct NotFound;

    Html(NotFound.render().unwrap())
}
