use crate::{routes::dirs::Directory, util};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
};
use futures::StreamExt as _;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::{future, io, path::PathBuf, time::SystemTime};
use tokio::fs;

pub fn routes() -> Router<super::RouterState> {
    fn serve_walls<F, T>(path: F) -> Router<super::RouterState>
    where
        F: Fn(State<super::RouterState>) -> T + Copy + Send + Sync + 'static,
        T: Directory + Send + Sync,
    {
        Router::new()
            .route(
                "/",
                get(async move |state, query| index(path(state), query).await),
            )
            .route("/random", get(async move |state| random(path(state)).await))
            .route(
                "/{filename}",
                get(async move |state, filename| specific(path(state), filename).await),
            )
    }
    Router::new()
        .merge(serve_walls(|s| s.dirs.walls().small()))
        .nest("/small", serve_walls(|s| s.dirs.walls().small()))
        .nest("/all", serve_walls(|s| s.dirs.walls().all()))
        .nest("/phone", serve_walls(|s| s.dirs.walls().phone()))
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

#[derive(Deserialize)]
struct IndexQuery {
    #[serde(default)]
    mtg: Option<bool>,
}

async fn index<D>(dir: D, query: Query<IndexQuery>) -> Result<impl IntoResponse, Error>
where
    D: Directory + Send + Sync,
{
    #[derive(Debug, Serialize)]
    struct Wallpaper {
        path: PathBuf,
        name: String,
        #[serde(skip)]
        date: SystemTime,
    }
    let dir = &dir;
    let mtg = query.mtg.unwrap_or(false);
    let mut images = util::fs::list_files_at(dir.get())
        .await?
        .filter(|path| {
            future::ready(
                mtg || !path
                    .file_name()
                    .unwrap() // always has a file name since it's from a read dir stream
                    .to_str()
                    .unwrap_or("")
                    .contains("mtg"),
            )
        })
        .then(|path| async move {
            let name = name_from_path(&path)?;
            io::Result::Ok(Wallpaper {
                date: fs::metadata(dir.file(path.to_str().unwrap()))
                    .await?
                    .modified()
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                path: path.to_owned(),
                name,
            })
        })
        .map(Result::ok)
        .filter_map(future::ready)
        .collect::<Vec<_>>()
        .await;
    images.sort_by_cached_key(|p| std::cmp::Reverse(p.date));
    Ok((StatusCode::OK, Json(images)))
}

async fn random(dir: impl Directory) -> Result<impl IntoResponse, Error> {
    Ok(util::fs::named_file(&util::fs::random_file(dir.get()).await?.path()).await?)
}

async fn specific(dir: impl Directory, filename: Path<String>) -> Result<impl IntoResponse, Error> {
    Ok(util::fs::named_file(&dir.file(&filename.0)).await?)
}

fn name_from_path(path: &std::path::Path) -> io::Result<String> {
    let mut name = path
        .file_stem()
        .ok_or(io::ErrorKind::InvalidData)?
        .to_str()
        .ok_or(io::ErrorKind::InvalidData)?
        .to_lowercase();

    for b in unsafe { name.as_bytes_mut() } {
        if *b == b'_' || *b == b'-' {
            *b = b' ';
        }
    }

    let name = name
        .trim()
        .trim_end_matches("wide")
        .trim_end_matches("vertical")
        .trim()
        .to_string();

    Ok(name)
}
