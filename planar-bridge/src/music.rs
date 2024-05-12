use std::io;

use actix_web::{
    web::{self, get, post},
    FromRequest, HttpResponse, ResponseError,
};
use askama_actix::Template;
use common::domain::{music_session::MusicSession, Hostname};
use futures::{future::LocalBoxFuture, FutureExt};
use reqwest::{
    header::{HeaderName, HeaderValue},
    StatusCode,
};
use serde::Deserialize;
use spark_protocol::{
    music::{MusicCmd, MusicCmdKind, Response},
    SuccessfulResponse,
};
use uuid::Uuid;

use crate::Backend;

pub fn routes() -> actix_web::Scope {
    web::scope("/music")
        .route("", get().to(index))
        .route("/current", get().to(now_playing))
        .route("/volume", get().to(volume))
        .route("/ctl", post().to(ctl))
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("unexpected response")]
    UnexpectedBackendResponse(String),
    #[error("player not found")]
    PlayerOrSessionNotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("badrequest")]
    BadRequest,
}

async fn request_from_backend(
    client: &Backend,
    target: &Target,
    cmd: MusicCmdKind,
) -> Result<spark_protocol::Response, Error> {
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

    Ok(response.error_for_status()?.json().await?)
}

impl ResponseError for Error {
    fn status_code(&self) -> reqwest::StatusCode {
        match self {
            Self::Io(_) | Self::Reqwest(_) | Self::UnexpectedBackendResponse(_) => {
                reqwest::StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::PlayerOrSessionNotFound => reqwest::StatusCode::NOT_FOUND,
            Self::Unauthorized => reqwest::StatusCode::UNAUTHORIZED,
            Self::BadRequest => reqwest::StatusCode::BAD_REQUEST,
        }
    }
}

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
    paused: bool,
    title: String,
    chapter: Option<spark_protocol::music::Chapter>,
    progress: f64,
}

async fn now_playing(backend: web::Data<Backend>, target: Target) -> Result<NowPlaying, Error> {
    let response = request_from_backend(&backend, &target, MusicCmdKind::Current).await?;

    let Ok(SuccessfulResponse::MusicResponse(Response::Current {
        paused,
        title,
        chapter,
        volume: _,
        progress,
    })) = response
    else {
        tracing::error!(?response, "unexpected backend response");
        return Err(Error::UnexpectedBackendResponse(format!("{response:?}")));
    };
    Ok(NowPlaying {
        paused,
        title,
        chapter,
        progress,
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
    let response = match response {
        Err(e) => return Err(Error::UnexpectedBackendResponse(format!("{e:?}"))),
        Ok(SuccessfulResponse::MusicResponse(r)) => r,
        Ok(r) => return Err(Error::UnexpectedBackendResponse(format!("{r:?}"))),
    };
    let res = match response {
        Response::PlayState { paused } => {
            HttpResponse::build(StatusCode::OK).body(PlayPause { paused }.render().unwrap())
        }
        Response::Title { title } => HttpResponse::build(StatusCode::OK).body(title),
        Response::Volume { volume } => HttpResponse::build(StatusCode::OK).body(volume.to_string()),
        Response::Current { .. } | Response::QueueSummary { .. } => {
            HttpResponse::build(StatusCode::OK)
                .insert_header((
                    HeaderName::from_static("hx-trigger"),
                    HeaderValue::from_static("new-current"),
                ))
                .body(())
        }
    };

    Ok(res)
}

async fn volume(backend: web::Data<Backend>, target: Target) -> Result<String, Error> {
    let response = request_from_backend(&backend, &target, MusicCmdKind::Current).await?;
    let Ok(SuccessfulResponse::MusicResponse(Response::Current { volume, .. })) = response else {
        return Err(Error::UnexpectedBackendResponse(format!("{response:?}")));
    };

    Ok(volume.to_string())
}
