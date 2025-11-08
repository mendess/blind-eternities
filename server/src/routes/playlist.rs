use crate::{auth, routes::dirs::Directory, util::fs::named_file};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
};
use common::playlist::{SONG_META_HEADER, SongId, SongMetadata};
use futures::TryStreamExt;
use http::{HeaderMap, StatusCode, header};
use std::{
    collections::HashMap,
    io,
    sync::{LazyLock, Mutex},
};
use tokio::{fs::File, process::Command};
use tokio_util::compat::FuturesAsyncReadCompatExt;

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .route("/", get(playlist))
        .route("/mtogo/version", get(mtogo_version))
        .route("/mtogo/download", get(mtogo_download))
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
    let audio_dir = st.dirs.music().audio();
    let file = search(&AUDIO_PATH_CACHE, &audio_dir, &id).await?;
    Ok((StatusCode::OK, file))
}

#[tracing::instrument(skip(st))]
async fn song_thumb(
    State(st): State<super::RouterState>,
    Path(id): Path<SongId>,
) -> Result<impl IntoResponse, Error> {
    let thumb_dir = st.dirs.music().thumb();
    let file = search(&THUMB_PATH_CACHE, &thumb_dir, &id).await?;
    Ok((StatusCode::OK, file))
}

#[tracing::instrument(skip(st))]
async fn song_meta(
    State(st): State<super::RouterState>,
    Path(id): Path<SongId>,
) -> Result<impl IntoResponse, Error> {
    let metadata = st.dirs.music().meta().file(&id).with_extension("json");

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

    let audio_dir = st.dirs.music().audio();
    let (id, mut file) = loop {
        let id = SongId::generate();
        let mut path = audio_dir.file(&id);
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
        st.dirs.music().meta().file(&id).with_extension("json"),
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
    if let Err(e) = search(&AUDIO_PATH_CACHE, &st.dirs.music().audio(), &id).await {
        return if e.kind() == io::ErrorKind::NotFound {
            Err(Error::BadRequest("corresponding audio file does not exist"))
        } else {
            Err(e.into())
        };
    };

    let ext = ext_from_headers(&headers)?;

    let thumb = st.dirs.music().mtogo().file(&id).with_extension(ext);
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

async fn search(
    cache: &'static Mutex<HashMap<String, String>>,
    // TODO: replace with impl when generics being excluded from use<> clauses becomes possible
    dir: &(dyn Directory + Sync),
    id: &str,
) -> io::Result<impl IntoResponse + use<>> {
    let mut path = dir.file(id);
    if let Some(ext) = cache.lock().unwrap().get(id) {
        path.set_extension(ext);
    } else {
        let mut read_dir = tokio::fs::read_dir(dir.get()).await?;
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

async fn mtogo_version(State(st): State<super::RouterState>) -> Result<impl IntoResponse, Error> {
    static PARSE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"versionCode='(?<vcode>\d+)' versionName='(?<version>\d+\.\d+\.\d+)'")
            .unwrap()
    });
    let output = Command::new(st.dirs.music().mtogo().file("aapt2"))
        .args(["dump", "badging"])
        .arg(st.dirs.music().mtogo().file("mtogo.apk"))
        .output()
        .await?;

    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|_| Error::Io(io::Error::other("aapt2 returned non utf8 bytes")))?;

    let Some(captures) = PARSE.captures(stdout) else {
        tracing::error!(?stdout, "invalid aapt2 output");
        return Err(Error::Io(io::Error::other("invalid appt2 output")));
    };

    let version_code = captures
        .name("vcode")
        .unwrap()
        .as_str()
        .parse::<u16>()
        .unwrap();

    let version = captures.name("version").unwrap().as_str();

    Ok((
        StatusCode::OK,
        axum::extract::Json(
            serde_json::json!({ "version_code": version_code, "version": version }),
        ),
    ))
}

async fn mtogo_download(State(st): State<super::RouterState>) -> Result<impl IntoResponse, Error> {
    Ok(named_file(&st.dirs.music().mtogo().file("mtogo.apk")).await?)
}
