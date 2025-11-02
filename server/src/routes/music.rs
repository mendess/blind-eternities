use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::post,
};
use http::StatusCode;
use spark_protocol::music::MusicCmdKind;

use crate::auth::{self, music_session::MusicSession};

pub fn routes() -> Router<super::RouterState> {
    Router::new().route("/ws/{id}", post(ws_message_music_player))
}

async fn ws_message_music_player(
    State(super::RouterState { socket_io, db, .. }): State<super::RouterState>,
    Path(id): Path<MusicSession>,
    Json(command): Json<MusicCmdKind>,
) -> impl IntoResponse {
    eprintln!("{id} :: {command:?}");
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
}
