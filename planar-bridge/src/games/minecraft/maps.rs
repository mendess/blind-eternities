use crate::games::minecraft::Error;
use askama::Template;
use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use common::web_server::named_file;
use serde::Deserialize;
use std::{io, path::Path, sync::Arc};
use tower_http::services::ServeDir;

pub fn routes(config: &super::Config) -> Router<Arc<super::Config>> {
    let router = Router::new()
        .route("/", get(maps))
        .route("/timelapse", get(timelapse))
        .route("/timelapse/{user}", get(timelapse_of))
        .route("/timelapse/{user}/{version}", get(timelapse_of));

    add_map_routes(
        router,
        config,
        &[
            ("/overworld/", "overworld-day"),
            ("/overworld/night/", "overworld-night"),
            ("/nether/", "nether"),
            ("/super-secret-map-nether-mid/", "nether-mid"),
            ("/super-secret-map-end/", "end"),
        ],
    )
}

fn add_map_routes(
    mut router: Router<Arc<super::Config>>,
    config: &super::Config,
    maps: &[(&str, &str)],
) -> Router<Arc<super::Config>> {
    let base = Path::new("map/web-export");
    for (map, dir) in maps {
        router = router
            .route(
                map.trim_end_matches("/"),
                get(Redirect::to(map.trim_start_matches("/"))),
            )
            .nest_service(
                map,
                ServeDir::new(config.backups_dir.join(base).join(dir))
                    .append_index_html_on_directories(true),
            );
    }
    router
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

async fn timelapse(config: State<Arc<super::Config>>) -> Result<impl IntoResponse, Error> {
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
    config: State<Arc<super::Config>>,
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
