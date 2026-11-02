mod deaths;
mod logs;
mod mods;

use crate::RouterState;
use askama::Template;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use common::web_server::named_file;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tower_http::services::ServeDir;

#[derive(Serialize, Deserialize)]
pub(super) struct Config {
    backups_dir: PathBuf,
    server_dir: PathBuf,
}

fn add_map_routes(
    mut router: Router<Arc<Config>>,
    config: &Config,
    maps: &[(&str, &str)],
) -> Router<Arc<Config>> {
    let base = Path::new("map/web-export");
    for (map, dir) in maps {
        router = router
            .route(map.trim_end_matches("/"), get(Redirect::to(map)))
            .nest_service(
                map,
                ServeDir::new(config.backups_dir.join(base).join(dir))
                    .append_index_html_on_directories(true),
            );
    }
    router
}

pub(super) fn router(config: Arc<Config>) -> Router<RouterState> {
    let router = Router::new()
        .route("/", get(index))
        .route("/deaths", get(deaths::deaths))
        .route("/mods", get(mods::get_mods))
        .route("/mods/large-biomes.mrpack", get(mods::generate_mod_pack))
        .route("/maps", get(maps))
        .route("/maps/timelapse", get(timelapse))
        .route("/maps/timelapse/{user}", get(timelapse_of))
        .route("/maps/timelapse/{user}/{version}", get(timelapse_of));
    let router = add_map_routes(
        router,
        &config,
        &[
            ("/maps/overworld/", "overworld-day"),
            ("/maps/overworld/night/", "overworld-night"),
            ("/maps/nether/", "nether"),
            ("/super-secret-map-nether-mid/", "nether-mid"),
        ],
    );
    router.with_state(config)
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

#[derive(Debug, Template)]
#[template(path = "games/minecraft/maps.html")]
struct Maps;

async fn maps() -> Result<impl IntoResponse, Error> {
    Ok(Html(Maps.render()?))
}

#[derive(Debug, Template)]
#[template(path = "games/minecraft/timelapse.html")]
struct Timelapse {
    all: String,
    trains: Vec<String>,
    players: Vec<String>,
}

async fn timelapse(config: State<Arc<Config>>) -> Result<impl IntoResponse, Error> {
    let timelapse_dir = config.backups_dir.join("map/timelapses");
    let mut timelapses = std::fs::read_dir(&timelapse_dir)?
        .map(|dir| {
            let dir = dir?;
            if dir.file_type()?.is_file() {
                Ok(None)
            } else {
                io::Result::Ok(Some(
                    dir.file_name()
                        .to_str()
                        .ok_or_else(|| io::Error::other("invalid file name"))?
                        .to_owned(),
                ))
            }
        })
        .filter_map(|d| d.transpose())
        .collect::<Result<Vec<_>, _>>()?;
    let all = {
        let all_idx = timelapses.iter().position(|p| p == "all").unwrap();
        timelapses.swap_remove(all_idx)
    };

    let mut trains = {
        let mut trains = Vec::new();
        while let Some(t) = timelapses.iter().position(|p| p.starts_with("train-")) {
            trains.push(timelapses.swap_remove(t));
        }
        trains
    };

    trains.sort();
    timelapses.sort_by_cached_key(|p| p.to_lowercase());

    Ok(Html(
        Timelapse {
            all,
            trains,
            players: timelapses,
        }
        .render()?,
    ))
}

#[derive(Debug, Template)]
#[template(path = "games/minecraft/timelapse_of.html")]
struct TimelapseOf {
    user: String,
}

#[derive(Deserialize)]
struct TimelapseOfPathParams {
    user: String,
    #[serde(default)]
    version: Option<String>,
}

async fn timelapse_of(
    config: State<Arc<Config>>,
    axum::extract::Path(params): axum::extract::Path<TimelapseOfPathParams>,
) -> Result<impl IntoResponse, Error> {
    let TimelapseOfPathParams { user, version } = params;
    if let Some(version) = version {
        let timelapse_dir = config.backups_dir.join("map/timelapses").join(user);
        Ok(match version.as_str() {
            "original" => named_file(timelapse_dir.join("full.mp4")).await,
            "4k" => named_file(timelapse_dir.join("4k.mp4")).await,
            "2k" => named_file(timelapse_dir.join("2k.mp4")).await,
            _ => return Err(Error::BadRequest),
        }
        .map(|r| r.into_response())?)
    } else {
        Ok(Html(TimelapseOf { user }.render()?).into_response())
    }
}
