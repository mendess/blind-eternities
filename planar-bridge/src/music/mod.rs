mod request_coalescing;

use std::{io, sync::Arc, time::Duration};

use askama_axum::Template;
use axum::{
    async_trait,
    extract::{FromRequestParts, Path, Query, State},
    response::{AppendHeaders, IntoResponse},
    routing::{get, post},
    Form, Json, Router,
};
use common::domain::{music_session::MusicSession, Hostname};
use futures::TryStreamExt;
use http::{header, HeaderMap, StatusCode};
use mappable_rc::Marc;
use mlib::{
    playlist::{PartialSearchResult, Playlist},
    queue::Current,
};
use serde::Deserialize;
use spark_protocol::{
    music::{MusicCmd, MusicCmdKind, Response},
    SuccessfulResponse,
};
use uuid::Uuid;

use crate::{cache, metrics, Backend};

use self::request_coalescing::{request_coalesced, SharedError};

pub fn routes() -> Router<Backend> {
    Router::new()
        .route("/", get(index))
        .route("/current", get(now_playing))
        .route("/volume", get(volume))
        .route("/ctl", post(ctl))
        .route("/tabs/:mode", get(tabs))
        .route("/now", get(now))
        .route("/search", post(search))
        .route("/queue", post(queue))
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("unexpected response")]
    UnexpectedBackendResponse(String),
    #[error("mlib error")]
    Mlib(#[from] mlib::Error),
    #[error("player not found")]
    PlayerOrSessionNotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request")]
    BadRequest,
    #[error("not found")]
    NotFound,
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) | Self::Reqwest(_) | Self::UnexpectedBackendResponse(_) | Self::Mlib(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::PlayerOrSessionNotFound | Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

async fn request_from_backend(
    client: &Backend,
    target: &Target,
    cmd: MusicCmdKind,
) -> Result<spark_protocol::music::Response, Error> {
    metrics::music_backend_request(&cmd);
    let request = match target {
        Target::Host { hostname, auth } => client
            .post(&format!("/persistent-connections/ws/send/{hostname}"))
            .expect("url should always parse")
            .bearer_auth(auth)
            .json(&spark_protocol::Command::Music(
                spark_protocol::music::MusicCmd {
                    command: cmd,
                    index: None,
                    username: None,
                },
            )),
        Target::Session { session } => client
            .post(&format!("/music/ws/{session}"))
            .expect("url should always parse")
            .json(&cmd),
    };
    let response = request.send().await?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(Error::PlayerOrSessionNotFound);
    }

    match response
        .error_for_status()?
        .json::<spark_protocol::Response>()
        .await?
    {
        Ok(SuccessfulResponse::MusicResponse(r)) => Ok(r),
        Ok(r) => Err(Error::UnexpectedBackendResponse(format!("{r:?}"))),
        Err(e) => Err(Error::UnexpectedBackendResponse(format!("{e:?}"))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Target {
    Host { hostname: Hostname, auth: Uuid },
    Session { session: MusicSession },
}

#[async_trait]
impl<S> FromRequestParts<S> for Target
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Target {
            Host { hostname: Hostname },
            Session { session: MusicSession },
        }

        let Query(target) = Query::<Target>::from_request_parts(parts, state)
            .await
            .map_err(|_| Error::BadRequest)?;
        let header_map = match HeaderMap::from_request_parts(parts, state).await {
            Ok(m) => m,
            Err(e) => match e {}, // infalible
        };

        match target {
            Target::Host { hostname } => header_map
                .get(header::AUTHORIZATION)
                .and_then(|a| a.to_str().ok()?.parse().ok())
                .map(|auth| Self::Host { hostname, auth })
                .ok_or(Error::Unauthorized),
            Target::Session { session } => Ok(Self::Session { session }),
        }
    }
}

impl Target {
    fn to_query_string(&self) -> String {
        match self {
            Self::Host { hostname, .. } => format!("hostname={hostname}"),
            Self::Session { session } => format!("session={session}"),
        }
    }
}

#[derive(Template)]
#[template(path = "music/index.html")]
struct MainPage {
    target: Target,
}

async fn index(target: Target) -> MainPage {
    MainPage { target }
}

#[derive(Template)]
#[allow(dead_code)]
#[template(path = "music/current.html")]
struct NowPlaying {
    current: Marc<Current>,
}

async fn now_playing(backend: State<Backend>, target: Target) -> Result<NowPlaying, SharedError> {
    Ok(NowPlaying {
        current: get_current(backend, target).await?,
    })
}

#[derive(Template)]
#[template(path = "music/playpause.html")]
struct PlayPause {
    paused: bool,
}

async fn ctl(
    backend: State<Backend>,
    target: Target,
    Json(cmd): Json<MusicCmd>,
) -> Result<impl IntoResponse, Error> {
    tracing::info!(?cmd, "ctl");
    let response = request_from_backend(&backend, &target, cmd.command).await?;
    let res = match response {
        Response::PlayState { paused } => {
            (StatusCode::OK, PlayPause { paused }.render().unwrap()).into_response()
        }
        Response::Title { title } => (StatusCode::OK, title).into_response(),
        Response::Volume { volume } => (StatusCode::OK, volume.to_string()).into_response(),
        Response::Current { .. } | Response::QueueSummary { .. } => (
            StatusCode::OK,
            AppendHeaders([("hx-trigger", "new-current")]),
        )
            .into_response(),
        Response::Now { .. } => StatusCode::BAD_REQUEST.into_response(),
    };

    Ok(res)
}

async fn volume(backend: State<Backend>, target: Target) -> Result<String, SharedError> {
    Ok(format!("{:.0}", get_current(backend, target).await?.volume))
}

enum Tab {
    Now,
    Queue,
}

#[derive(Template)]
#[template(path = "music/tabs.html")]
struct Tabs {
    target: Target,
    tab: Tab,
}

async fn tabs(target: Target, kind: Path<String>) -> Result<Tabs, Error> {
    let tab = match kind.as_str() {
        "now" => Tab::Now,
        "queue" => Tab::Queue,
        _ => return Err(Error::NotFound),
    };
    Ok(Tabs { target, tab })
}

#[derive(Template)]
#[template(path = "music/now.html")]
struct Now {
    now: Marc<(Vec<String>, String, Vec<String>)>,
}

async fn now(backend: State<Backend>, target: Target) -> Result<Now, Error> {
    let now = cache::get_or_init(
        &target.to_query_string(),
        || async {
            let response =
                request_from_backend(&backend, &target, MusicCmdKind::Now { amount: Some(20) })
                    .await?;
            let Response::Now {
                before,
                current,
                after,
            } = response
            else {
                return Err(Error::UnexpectedBackendResponse(format!("{response:?}")));
            };
            Ok((before, current, after))
        },
        Duration::from_secs(1),
    )
    .await?;
    Ok(Now { now })
}

#[derive(Template)]
#[template(path = "music/search-results.html")]
struct SearchResults {
    songs: Vec<String>,
    target: Target,
}

#[derive(Deserialize, Debug)]
struct SearchFormData {
    search: String,
}

async fn search(
    target: Target,
    Form(SearchFormData { search }): Form<SearchFormData>,
) -> Result<SearchResults, Error> {
    let playlist = load_playlist().await?;
    let mut songs = if search.is_empty() {
        playlist.songs.iter().map(|s| s.name.clone()).collect()
    } else {
        match playlist.partial_name_search(search.split_whitespace()) {
            PartialSearchResult::None => vec![],
            PartialSearchResult::One(index) => vec![index.name.clone()],
            PartialSearchResult::Many(names) => names,
        }
    };

    songs.insert(0, search);

    Ok(SearchResults { songs, target })
}

async fn get_current(
    backend: State<Backend>,
    target: Target,
) -> Result<Marc<Current>, SharedError> {
    cache::get_or_init(
        &target.to_query_string(),
        || async {
            let response = request_coalesced(&backend, target, MusicCmdKind::Current).await;
            match response {
                Ok(Response::Current { current }) => Ok(current.clone()),
                Ok(_) => {
                    tracing::error!(?response, "unexpected backend response");
                    Err(Arc::new(Error::UnexpectedBackendResponse(format!("{response:?}"))).into())
                }
                Err(e) => Err(e),
            }
        },
        Duration::from_secs(1),
    )
    .await
}

async fn load_playlist() -> Result<Marc<Playlist>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

    async fn init() -> Result<Playlist, Error> {
        let playlist_request = reqwest::get(
            "https://raw.githubusercontent.com/mendess/spell-book/master/runes/m/playlist",
        )
        .await?;

        let stream = tokio_util::io::StreamReader::new(
            playlist_request.bytes_stream().map_err(io::Error::other),
        );

        Ok(Playlist::load_from_reader(stream).await?)
    }

    cache::get_or_init(Default::default(), init, ONE_HOUR).await
}

#[derive(Deserialize, Debug)]
struct QueueCommand {
    query: String,
    search: bool,
}

#[derive(Template)]
#[template(
    source = "<span>queued behind {{ distance }} songs!</span>",
    ext = "html"
)]
struct QueueSummary {
    distance: usize,
}

async fn queue(
    backend: State<Backend>,
    target: Target,
    Json(QueueCommand { query, search }): Json<QueueCommand>,
) -> Result<QueueSummary, Error> {
    let response =
        request_from_backend(&backend, &target, MusicCmdKind::Queue { query, search }).await?;
    let Response::QueueSummary {
        moved_to, current, ..
    } = response
    else {
        return Err(Error::UnexpectedBackendResponse(format!("{response:?}")));
    };
    println!("queueing {search}");
    Ok(QueueSummary {
        distance: moved_to.saturating_sub(current),
    })
}
