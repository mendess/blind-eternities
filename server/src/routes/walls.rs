use crate::{
    routes::{RouterState, dirs::Directory},
    util,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
};
use futures::StreamExt as _;
use http::StatusCode;
use image::{
    AnimationDecoder,
    codecs::{
        gif::{GifDecoder, GifEncoder},
        jpeg::JpegEncoder,
    },
    imageops::{FilterType, resize},
};
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
            .route(
                "/thumb/{filename}",
                get(async move |state: State<RouterState>, filename| {
                    thumb(state.clone(), path(state), filename).await
                }),
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
    #[error("bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::BadRequest(e) => (StatusCode::BAD_REQUEST, e.to_string()),
        }
        .into_response()
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

async fn thumb(
    State(st): State<RouterState>,
    dir: impl Directory,
    filename: Path<String>,
) -> Result<impl IntoResponse, Error> {
    let thumb_path = st.dirs.walls().thumb().file(&filename);
    match util::fs::named_file(&thumb_path).await {
        Ok(file) => Ok(file.into_response()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(create_thumb(dir.file(&filename), thumb_path)
                .await
                .map_err(io::Error::other)?
                .map(IntoResponse::into_response)
                .ok_or_else(|| Error::BadRequest("unsupported extension".to_string()))?)
        }
        Err(e) => Err(e.into()),
    }
}

#[tracing::instrument]
async fn create_thumb(source: PathBuf, dest: PathBuf) -> io::Result<Option<impl IntoResponse>> {
    tracing::info!("generating thumbnail");
    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
    match source
        .extension()
        .unwrap_or(std::ffi::OsStr::new(""))
        .to_str()
        .unwrap()
        .to_lowercase()
        .as_str()
    {
        "gif" => {
            compress_gif(source, &dest)
                .await
                .map_err(io::Error::other)?;
            Ok(Some(util::fs::named_file(&dest).await?))
        }
        "jpg" | "jpeg" | "png" | "bmp" => {
            compress_image(source, &dest)
                .await
                .map_err(io::Error::other)?;
            Ok(Some(util::fs::named_file(&dest).await?))
        }
        _ => Ok(None),
    }
}

fn thumb_resolution(name: &std::ffi::OsStr) -> (u32, u32) {
    let name = name.to_str().unwrap();
    if name.contains("wide") {
        (723, 200)
    } else if name.contains("vertical") {
        (356, 440)
    } else {
        (356, 200)
    }
}

async fn compress_gif(source: PathBuf, dest: &std::path::Path) -> anyhow::Result<()> {
    let dest = dest.to_owned();
    tokio::task::spawn_blocking(move || {
        let og_pic = GifDecoder::new(std::io::BufReader::new(std::fs::File::open(source)?))?;

        let (w, h) = thumb_resolution(dest.as_os_str());

        let frames = og_pic.into_frames().map(|f| {
            let mut f = f?;
            resize(f.buffer_mut(), w, h, FilterType::CatmullRom);
            Ok(f)
        });

        let buffer = std::fs::File::create(dest)?;
        let mut encoder = GifEncoder::new(buffer);
        encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
        encoder.try_encode_frames(frames)?;
        Ok(())
    })
    .await
    .unwrap()
}

async fn compress_image(source: PathBuf, dest: &std::path::Path) -> anyhow::Result<()> {
    let dest = dest.to_owned();
    tokio::task::spawn_blocking(move || {
        let og_pic = image::open(source)?;
        let mut buffer = std::fs::File::create(&dest)?;
        let (w, h) = thumb_resolution(dest.as_os_str());
        JpegEncoder::new_with_quality(&mut buffer, 60).encode_image(&og_pic.thumbnail(w, h))?;
        Ok(())
    })
    .await
    .unwrap()
}
