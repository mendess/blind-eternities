use std::{io, time::Duration};

use actix_web::{
    http::StatusCode,
    web::{self, get, post},
    FromRequest, HttpResponse, ResponseError,
};
use askama_actix::Template;
use common::domain::{music_session::MusicSession, Hostname};
use futures::{future::LocalBoxFuture, FutureExt, TryStreamExt};
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

pub fn routes() -> actix_web::Scope {
    web::scope("/music")
        .route("", get().to(index))
        .route("/current", get().to(now_playing))
        .route("/volume", get().to(volume))
        .route("/ctl", post().to(ctl))
        .route("/tabs/{mode}", get().to(tabs))
        .route("/now", get().to(now))
        .route("/search", post().to(search))
        .route("/queue", post().to(queue))
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

async fn request_from_backend(
    client: &Backend,
    target: &Target,
    cmd: MusicCmdKind,
) -> Result<spark_protocol::music::Response, Error> {
    metrics::music_backend_request(&cmd);
    let request = match target {
        Target::Host { hostname, auth } => client
            .post(&format!("/persistent-connections/send/{hostname}"))
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
            .post(&format!("/music/{session}"))
            .expect("url should always parse")
            .json(&cmd),
    };
    let response = request.send().await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
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

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
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

#[derive(Debug)]
enum Target {
    Host { hostname: Hostname, auth: Uuid },
    Session { session: MusicSession },
}

impl FromRequest for Target {
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;
    type Error = Error;
    fn from_request(
        req: &actix_web::HttpRequest,
        payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Target {
            Host { hostname: Hostname },
            Session { session: MusicSession },
        }

        let target_parser = web::Query::<Target>::from_request(req, payload);
        let req = req.clone();
        async move {
            let target = target_parser.await.map_err(|_| Error::BadRequest)?;

            match target.into_inner() {
                Target::Host { hostname } => req
                    .headers()
                    .get("authorization")
                    .and_then(|h| h.to_str().ok().and_then(|h| h.parse().ok()))
                    .map(|auth| Self::Host { hostname, auth })
                    .ok_or_else(|| Error::Unauthorized),
                Target::Session { session } => Ok(Self::Session { session }),
            }
        }
        .boxed_local()
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

async fn now_playing(backend: web::Data<Backend>, target: Target) -> Result<NowPlaying, Error> {
    Ok(NowPlaying {
        current: get_current(backend, &target).await?,
    })
}

#[derive(Template)]
#[template(path = "music/playpause.html")]
struct PlayPause {
    paused: bool,
}

async fn ctl(
    backend: web::Data<Backend>,
    target: Target,
    cmd: web::Json<MusicCmd>,
) -> Result<actix_web::HttpResponse, Error> {
    tracing::info!(?cmd, "ctl");
    let response = request_from_backend(&backend, &target, cmd.into_inner().command).await?;
    let res = match response {
        Response::PlayState { paused } => {
            HttpResponse::build(StatusCode::OK).body(PlayPause { paused }.render().unwrap())
        }
        Response::Title { title } => HttpResponse::build(StatusCode::OK).body(title),
        Response::Volume { volume } => HttpResponse::build(StatusCode::OK).body(volume.to_string()),
        Response::Current { .. } | Response::QueueSummary { .. } => {
            HttpResponse::build(StatusCode::OK)
                .insert_header(("hx-trigger", "new-current"))
                .body(())
        }
        Response::Now { .. } => HttpResponse::build(StatusCode::BAD_REQUEST).into(),
    };

    Ok(res)
}

async fn volume(backend: web::Data<Backend>, target: Target) -> Result<String, Error> {
    Ok(get_current(backend, &target).await?.volume.to_string())
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

async fn tabs(target: Target, kind: web::Path<String>) -> Result<Tabs, Error> {
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

async fn now(backend: web::Data<Backend>, target: Target) -> Result<Now, Error> {
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
    web::Form(SearchFormData { search }): web::Form<SearchFormData>,
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

async fn get_current(backend: web::Data<Backend>, target: &Target) -> Result<Marc<Current>, Error> {
    cache::get_or_init(
        &target.to_query_string(),
        || async {
            let response = request_from_backend(&backend, target, MusicCmdKind::Current).await?;

            let Response::Current { current } = response else {
                tracing::error!(?response, "unexpected backend response");
                return Err(Error::UnexpectedBackendResponse(format!("{response:?}")));
            };

            Ok(current)
        },
        Duration::from_millis(1),
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
    backend: web::Data<Backend>,
    target: Target,
    web::Json(QueueCommand { query, search }): web::Json<QueueCommand>,
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
