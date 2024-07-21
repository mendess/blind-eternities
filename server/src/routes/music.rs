use actix_http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use spark_protocol::music::MusicCmdKind;
use sqlx::PgPool;

use crate::{
    auth::music_session::MusicSession,
    persistent_connections::{self, Connections},
};

pub fn routes() -> actix_web::Scope {
    web::scope("/music").route("/{id}", web::post().to(message_music_player))
}

#[derive(Debug, thiserror::Error)]
enum MusicError {
    #[error(transparent)]
    ConnectionError(#[from] persistent_connections::ConnectionError),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
}

impl ResponseError for MusicError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ConnectionError(persistent_connections::ConnectionError::NotFound) => {
                StatusCode::NOT_FOUND
            }
            Self::ConnectionError(persistent_connections::ConnectionError::ConnectionDropped(
                _,
            )) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::SqlxError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[tracing::instrument(skip(db, connections))]
async fn message_music_player(
    db: web::Data<PgPool>,
    connections: web::Data<Connections>,
    id: web::Path<MusicSession>,
    command: web::Json<MusicCmdKind>,
) -> Result<HttpResponse, MusicError> {
    let Some(hostname) = id.hostname(&db).await? else {
        return Ok(HttpResponse::Unauthorized().into());
    };

    let response = connections.request(&hostname, command.into_inner()).await?;

    Ok(HttpResponse::Ok().json(response))
}
