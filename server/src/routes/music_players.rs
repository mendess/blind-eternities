use actix_web::{http::StatusCode, web, HttpResponse, ResponseError};
use common::domain::Hostname;
use spark_protocol::{
    music::{MusicCmd, MusicCmdKind},
    Local,
};
use tracing::instrument;

use crate::persistent_connections::{ConnectionError, Connections};

pub fn routes() -> actix_web::Scope {
    web::scope("/music").service(
        web::scope("/players/{hostname}")
            .route("/frwd", web::get().to(skip_forward))
            .route("/back", web::get().to(skip_backward))
            .route("/change-volume", web::get().to(change_volume))
            .route("/cycle-pause", web::get().to(cycle_pause))
            .route("/queue", web::post().to(queue))
            .route("/current", web::get().to(current)),
    )
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MusicPlayersError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    #[error("connection error")]
    ConnectionError(#[from] ConnectionError),
}

impl ResponseError for MusicPlayersError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ConnectionError(ConnectionError::NotFound) => StatusCode::NOT_FOUND,
            Self::ConnectionError(ConnectionError::ConnectionDropped) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

#[instrument(name = "default player")]
async fn current(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
) -> Result<HttpResponse, MusicPlayersError> {
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::Current,
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}

#[instrument(name = "skip forward")]
async fn skip_forward(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
) -> Result<HttpResponse, MusicPlayersError> {
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::Frwd,
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}

#[instrument(name = "skip backward")]
async fn skip_backward(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
) -> Result<HttpResponse, MusicPlayersError> {
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::Back,
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, serde::Deserialize)]
struct Amount {
    a: i32,
}

#[instrument(name = "change volume")]
async fn change_volume(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
    amount: web::Query<Amount>,
) -> Result<HttpResponse, MusicPlayersError> {
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::ChangeVolume { amount: amount.a },
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}

#[instrument(name = "cycle pause")]
async fn cycle_pause(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
) -> Result<HttpResponse, MusicPlayersError> {
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::CyclePause,
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, serde::Deserialize)]
struct QueueRequest {
    query: String,
    search: bool,
}

#[instrument(name = "queue")]
async fn queue(
    conn: web::Data<Connections>,
    hostname: web::Path<Hostname>,
    body: web::Json<QueueRequest>,
) -> Result<HttpResponse, MusicPlayersError> {
    let body = body.into_inner();
    let response = conn
        .request(
            &hostname,
            Local::Music(MusicCmd {
                index: None,
                command: MusicCmdKind::Queue {
                    query: body.query.into(),
                    search: body.search,
                },
            }),
        )
        .await?;
    Ok(HttpResponse::Ok().json(response))
}
