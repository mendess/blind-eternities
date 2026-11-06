mod deaths;
mod logs;
mod maps;
mod mods;

use crate::RouterState;
use askama::Template;
use axum::{
    Router,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, sync::Arc};

#[derive(Serialize, Deserialize)]
pub(super) struct Config {
    backups_dir: PathBuf,
    server_dir: PathBuf,
}

pub(super) fn router(config: Arc<Config>) -> Router<RouterState> {
    Router::new()
        .route("/", get(index))
        .route("/deaths", get(deaths::deaths))
        .route("/mods", get(mods::get_mods))
        .route("/mods/large-biomes.mrpack", get(mods::generate_mod_pack))
        .nest("/maps", maps::routes(&config))
        .with_state(config)
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("rendering: {0}")]
    Rendering(#[from] askama::Error),
    #[error("bad request")]
    BadRequest,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            Error::Io(_) | Error::Serde(_) | Error::Rendering(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Error::BadRequest => StatusCode::BAD_REQUEST,
        };
        (status, self.to_string()).into_response()
    }
}

#[derive(Debug, Template)]
#[template(path = "games/minecraft/index.html")]
struct Index;

async fn index() -> Result<impl IntoResponse, Error> {
    Ok(Html(Index.render()?))
}
