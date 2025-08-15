use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
};
use common::domain::{Hostname, music_session::ExpiresAt};
use http::StatusCode;
use sqlx::PgPool;

use crate::auth::{self, music_session::MusicSession};

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .route("/health_check", get(health_check))
        .route(
            "/music-session/{hostname}",
            get(create_music_session).delete(delete_music_session),
        )
}

async fn health_check(_: auth::Admin) -> StatusCode {
    StatusCode::OK
}

#[derive(thiserror::Error, Debug)]
pub enum MusicSessionError {
    #[error(transparent)]
    AuthError(#[from] auth::AuthError),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
}

impl IntoResponse for MusicSessionError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::AuthError(a) => a.into_response(),
            Self::SqlxError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
        }
    }
}

#[tracing::instrument(skip(db))]
async fn create_music_session(
    _: auth::Admin,
    db: State<Arc<PgPool>>,
    Path(hostname): Path<Hostname>,
    Query(ExpiresAt { expires_at }): Query<ExpiresAt>,
) -> Result<impl IntoResponse, MusicSessionError> {
    let id = MusicSession::create_for(db.as_ref(), &hostname, expires_at).await?;
    tracing::info!("created id = {id}");

    Ok(Json(id))
}

#[tracing::instrument(skip(db))]
async fn delete_music_session(
    _: auth::Admin,
    db: State<Arc<PgPool>>,
    Path(id): Path<MusicSession>,
) -> Result<impl IntoResponse, MusicSessionError> {
    id.delete(&db).await?;
    Ok(StatusCode::OK)
}
