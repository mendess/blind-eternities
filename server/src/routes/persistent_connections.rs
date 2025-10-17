use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
};
use common::{domain::Hostname, ws};
use http::StatusCode;
use socketioxide::{AckError, SendError, SocketError, extract::SocketRef};

use crate::{
    auth,
    persistent_connections::{
        Generation,
        ws::{SHostname, SocketIo},
    },
};

pub fn routes() -> Router<super::RouterState> {
    Router::new().nest(
        "/ws",
        Router::new()
            .route("/", get(ws_list_persistent_connections))
            .route("/send/{hostname}", post(ws_send)),
    )
}

async fn ws_list_persistent_connections(
    _: auth::Admin,
    State(io): State<SocketIo>,
) -> impl IntoResponse {
    let connected = io.of(ws::NS).unwrap().sockets();

    (
        StatusCode::OK,
        Json(
            connected
                .into_iter()
                .filter_map(|s| s.extensions.get::<SHostname>())
                .collect::<Vec<_>>(),
        ),
    )
}

pub async fn ws_send(
    _: auth::Admin,
    State(io): State<SocketIo>,
    hostname: Path<Hostname>,
    Json(command): Json<spark_protocol::Command>,
) -> axum::response::Response {
    let sockets = io.of(ws::NS).unwrap().sockets();
    tracing::warn!("socket#: {}", sockets.len());
    let by_hostname = |s: &SocketRef| {
        s.extensions
            .get::<SHostname>()
            .is_some_and(|h| *h == *hostname)
    };
    let generation = |s: &SocketRef| s.extensions.get::<Generation>().unwrap();
    let socket = match sockets
        .into_iter()
        .filter(by_hostname)
        .max_by_key(generation)
    {
        Some(ns) => ns,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    tracing::info!(?command, "sending message to ws");
    let emit_future = socket
        .timeout(Duration::from_secs(60))
        .emit_with_ack::<_, [spark_protocol::Response; 1]>(ws::COMMAND, &command);
    let response = match emit_future {
        Ok(future) => future.await,
        Err(SendError::Socket(SocketError::Closed)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "socket closed").into_response();
        }
        Err(SendError::Socket(SocketError::InternalChannelFull)) => {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        Err(SendError::Serialize(e)) => {
            panic!("should never fail to serialize a command: {e:?}")
        }
    };

    tracing::info!(?response, "received response");

    match response {
        Ok([data]) => (StatusCode::OK, Json(data)).into_response(),
        Err(AckError::Timeout) => StatusCode::GATEWAY_TIMEOUT.into_response(),
        Err(AckError::Decode(e)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(AckError::Socket(SocketError::Closed)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "socket closed").into_response()
        }
        Err(AckError::Socket(SocketError::InternalChannelFull)) => {
            StatusCode::TOO_MANY_REQUESTS.into_response()
        }
    }
}
