use crate::{auth, routes::PlaylistConfig};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    response::{AppendHeaders, IntoResponse},
    routing::{get, post},
};
use common::playlist::{SONG_META_HEADER, SongId, SongMetadata};
use futures::TryStreamExt;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use httpdate::fmt_http_date;
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{LazyLock, Mutex},
    time::SystemTime,
};
use tokio::fs::File;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::io::ReaderStream;

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .route("/", get(playlist))
        .route("/song/audio", post(add_song))
        .route("/song/audio/{id}", get(song_audio))
        .route("/song/thumb/{id}", get(song_thumb).post(add_thumb))
        .route("/song/metadata/{id}", get(song_meta))
}

static AUDIO_PATH_CACHE: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);
static THUMB_PATH_CACHE: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("bad request: {0}")]
    BadRequest(&'static str),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let code = match self {
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        };
        (code, self.to_string()).into_response()
    }
}

macro_rules! mkdir {
    ($dir:expr) => {{
        static CREATE_DIR: std::sync::Once = std::sync::Once::new();
        let path = $dir;
        CREATE_DIR.call_once(|| {
            std::fs::create_dir_all(&path).unwrap();
        });
        path
    }};
}

impl PlaylistConfig {
    fn audio_dir(&self) -> PathBuf {
        mkdir!(self.song_dir.join("audio"))
    }
    fn meta_dir(&self) -> PathBuf {
        mkdir!(self.song_dir.join("meta"))
    }
    fn thumb_dir(&self) -> PathBuf {
        mkdir!(self.song_dir.join("thumb"))
    }
}

async fn playlist() -> Result<impl IntoResponse, Error> {
    reqwest::get(
        "https://raw.githubusercontent.com/mendess/spell-book/master/runes/m/playlist.json",
    )
    .await
    .map_err(|e| Error::Io(io::Error::other(e)))
    .and_then(|mut r| {
        let mut response = axum::response::Response::builder().status(r.status());
        *response.headers_mut().unwrap() = std::mem::take(r.headers_mut());
        response
            .body(axum::body::Body::from_stream(r.bytes_stream()))
            .map_err(|e| Error::Io(io::Error::other(e)))
    })
}

#[tracing::instrument(skip(st))]
async fn song_audio(
    State(st): State<super::RouterState>,
    Path(id): Path<SongId>,
) -> Result<impl IntoResponse, Error> {
    let audio_dir = st.playlist_config.audio_dir();
    let file = search(&AUDIO_PATH_CACHE, &audio_dir, &id).await?;
    Ok((StatusCode::OK, file))
}

#[tracing::instrument(skip(st))]
async fn song_thumb(
    State(st): State<super::RouterState>,
    Path(id): Path<SongId>,
) -> Result<impl IntoResponse, Error> {
    let thumb_dir = st.playlist_config.thumb_dir();
    let file = search(&THUMB_PATH_CACHE, &thumb_dir, &id).await?;
    Ok((StatusCode::OK, file))
}

#[tracing::instrument(skip(st))]
async fn song_meta(
    State(st): State<super::RouterState>,
    Path(id): Path<SongId>,
) -> Result<impl IntoResponse, Error> {
    let metadata = st
        .playlist_config
        .meta_dir()
        .join(id)
        .with_extension("json");

    Ok((StatusCode::OK, named_file(&metadata).await?))
}

#[tracing::instrument(skip(st, song))]
async fn add_song(
    _: auth::Admin,
    State(st): State<super::RouterState>,
    headers: HeaderMap,
    song: Body,
) -> Result<impl IntoResponse, Error> {
    let Some(meta) = headers.get(SONG_META_HEADER) else {
        return Err(Error::BadRequest("missing song metadata"));
    };

    let metadata = serde_json::from_slice::<SongMetadata>(meta.as_bytes())
        .map_err(|_| Error::BadRequest("song meta not formatted correctly"))?;

    let ext = ext_from_headers(&headers)?;

    let audio_dir = st.playlist_config.audio_dir();
    let (id, mut file) = loop {
        let id = SongId::generate();
        let mut path = audio_dir.join(&id);
        path.set_extension(ext);
        match File::create_new(path).await {
            Ok(f) => break (id, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    };

    tokio::io::copy(
        &mut song
            .into_data_stream()
            .map_err(io::Error::other)
            .into_async_read()
            .compat(),
        &mut file,
    )
    .await?;
    tokio::fs::write(
        st.playlist_config
            .meta_dir()
            .join(&id)
            .with_extension("json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .await?;

    Ok((StatusCode::OK, Json(id)))
}

#[tracing::instrument(skip(st, body))]
async fn add_thumb(
    _: auth::Admin,
    State(st): State<super::RouterState>,
    headers: HeaderMap,
    Path(id): Path<SongId>,
    body: Body,
) -> Result<impl IntoResponse, Error> {
    // assert music file exists
    if let Err(e) = search(&AUDIO_PATH_CACHE, &st.playlist_config.audio_dir(), &id).await {
        return if e.kind() == io::ErrorKind::NotFound {
            Err(Error::BadRequest("corresponding audio file does not exist"))
        } else {
            Err(e.into())
        };
    };

    let ext = ext_from_headers(&headers)?;

    let thumb = st.playlist_config.thumb_dir().join(id).with_extension(ext);
    let mut file = File::create(thumb).await?;
    tokio::io::copy(
        &mut body
            .into_data_stream()
            .map_err(io::Error::other)
            .into_async_read()
            .compat(),
        &mut file,
    )
    .await?;

    Ok(StatusCode::OK)
}

fn ext_from_headers(headers: &HeaderMap) -> Result<&'static str, Error> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Err(Error::BadRequest("missing content type"));
    };

    content_type
        .to_str()
        .map_err(|_| Error::BadRequest("content type not utf8"))?
        .split_once("/")
        .and_then(|(top, sub)| mime_guess::get_extensions(top, sub))
        .and_then(|exts| exts.first().copied())
        .ok_or(Error::BadRequest("invalid content type"))
}

pub async fn named_file(path: &std::path::Path) -> io::Result<impl IntoResponse + use<>> {
    let file = File::open(path).await?;
    let meta = file.metadata().await?;
    let len = meta.len();
    let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let filename = path.file_name().unwrap().to_string_lossy();

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let headers = AppendHeaders([
        (
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref()).unwrap(),
        ),
        (
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&len.to_string()).unwrap(),
        ),
        // (
        //     header::ACCEPT_RANGES,
        //     const { HeaderValue::from_static("bytes") },
        // ),
        (
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("inline; filename=\"{}\"", filename)).unwrap(),
        ),
        (
            header::LAST_MODIFIED,
            HeaderValue::from_str(&fmt_http_date(modified)).unwrap(),
        ),
        (
            header::DATE,
            HeaderValue::from_str(&fmt_http_date(SystemTime::now())).unwrap(),
        ),
        (header::ETAG, {
            let dur = modified.duration_since(SystemTime::UNIX_EPOCH).unwrap();
            // Simple ETag: "<len>:<modified>"
            let etag = format!("\"{:x}:{:x}\"", len, dur.as_secs());
            HeaderValue::from_str(&etag).unwrap()
        }),
    ]);

    Ok((headers, body))
}

async fn search(
    cache: &'static Mutex<HashMap<String, String>>,
    dir: &std::path::Path,
    id: &str,
) -> io::Result<impl IntoResponse + use<>> {
    let mut path = dir.join(id);
    if let Some(ext) = cache.lock().unwrap().get(id) {
        path.set_extension(ext);
    } else {
        let mut read_dir = tokio::fs::read_dir(&dir).await?;
        while let Some(f) = read_dir.next_entry().await? {
            let f_path = f.path();
            if f_path
                .file_stem()
                .is_some_and(|s| s.as_encoded_bytes().starts_with(id.as_bytes()))
            {
                let ext = f_path.extension().unwrap_or_default().to_str().unwrap();
                cache.lock().unwrap().insert(id.to_owned(), ext.to_owned());
                path.set_extension(ext);
                break;
            }
        }
    }
    named_file(&path).await
}
