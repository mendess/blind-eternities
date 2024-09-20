use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use http::StatusCode;
use spark_protocol::music::MusicCmdKind;

use crate::{
    auth::{self, music_session::MusicSession},
    persistent_connections,
};

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .route("/ws/:id", post(ws_message_music_player))
        .route("/:id", post(message_music_player))
}

#[derive(Debug, thiserror::Error)]
enum MusicError {
    #[error("unauthorized")]
    Unauthorized,
    #[error(transparent)]
    ConnectionError(#[from] persistent_connections::ConnectionError),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
}

impl IntoResponse for MusicError {
    fn into_response(self) -> axum::response::Response {
        let code = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::ConnectionError(persistent_connections::ConnectionError::NotFound) => {
                StatusCode::NOT_FOUND
            }
            Self::ConnectionError(persistent_connections::ConnectionError::ConnectionDropped(
                _,
            )) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::SqlxError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (code, self.to_string()).into_response()
    }
}

async fn message_music_player(
    State(super::RouterState {
        connections, db, ..
    }): State<super::RouterState>,
    Path(id): Path<MusicSession>,
    Json(command): Json<MusicCmdKind>,
) -> Result<impl IntoResponse, MusicError> {
    let Some(hostname) = id.hostname(&db).await? else {
        return Err(MusicError::Unauthorized);
    };

    let response = connections.request(&hostname, command).await?;

    Ok((StatusCode::OK, Json(response)))
}

async fn ws_message_music_player(
    State(super::RouterState { socket_io, db, .. }): State<super::RouterState>,
    Path(id): Path<MusicSession>,
    Json(command): Json<MusicCmdKind>,
) -> impl IntoResponse {
    let hostname = match id.hostname(&db).await {
        Ok(Some(h)) => h,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    super::persistent_connections::ws_send(
        auth::Admin {},
        State(socket_io),
        Path(hostname),
        Json(command.into()),
    )
    .await
    .into_response()
}
