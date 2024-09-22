use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use common::{domain::Hostname, ws};
use http::StatusCode;
use socketioxide::{
    ack::AckResponse, extract::SocketRef, AckError, AdapterError, SendError, SocketError,
};

use crate::{
    auth,
    persistent_connections::{
        connections::Generation,
        ws::{SHostname, SocketIo},
        ConnectionError, Connections,
    },
};

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .nest(
            "/ws",
            Router::new()
                .route("/", get(ws_list_persistent_connections))
                .route("/send/:hostname", post(ws_send)),
        )
        .route("/", get(list_persistent_connections))
        .route("/send/:hostname", post(send))
}

async fn ws_list_persistent_connections(
    _: auth::Admin,
    State(io): State<SocketIo>,
) -> impl IntoResponse {
    let connected = match io.of(ws::NS).unwrap().sockets() {
        Ok(rooms) => rooms,
        Err(infallible) => match infallible {},
    };

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
) -> impl IntoResponse {
    let sockets = match io.of(ws::NS).unwrap().sockets() {
        Ok(sockets) => sockets,
        Err(infallible) => match infallible {},
    };
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
        .emit_with_ack::<_, [spark_protocol::Response; 1]>(ws::COMMAND, command);
    let response = match emit_future {
        Ok(future) => future.await,
        Err(SendError::Socket(SocketError::Closed(_))) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "socket closed").into_response()
        }
        Err(SendError::Socket(SocketError::InternalChannelFull(_))) => {
            return StatusCode::TOO_MANY_REQUESTS.into_response()
        }
        Err(SendError::Serialize(e)) => {
            panic!("should never fail to serialize a command: {e:?}")
        }
    };

    tracing::info!(?response, "received response");

    match response {
        Ok(AckResponse { data: [data], .. }) => (StatusCode::OK, Json(data)).into_response(),
        Err(AckError::Timeout) => StatusCode::GATEWAY_TIMEOUT.into_response(),
        Err(AckError::Serde(e)) => {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(AckError::Socket(SocketError::Closed(_))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "socket closed").into_response()
        }
        Err(AckError::Socket(SocketError::InternalChannelFull(_))) => {
            StatusCode::TOO_MANY_REQUESTS.into_response()
        }
        Err(AckError::Adapter(AdapterError(infallible))) => {
            panic!("LocalAdapter should never error: {infallible}")
        }
    }
}

async fn list_persistent_connections(
    _: auth::Admin,
    connections: State<Arc<Connections>>,
) -> impl IntoResponse {
    let connected = connections.connected_hosts().await;
    (
        StatusCode::OK,
        Json(connected.into_iter().map(|(h, _)| h).collect::<Vec<_>>()),
    )
}

async fn send(
    _: auth::Admin,
    connections: State<Arc<Connections>>,
    hostname: Path<Hostname>,
    Json(command): Json<spark_protocol::Command>,
) -> impl IntoResponse {
    let r = connections.request(&hostname, command).await;
    tracing::debug!(response = ?r, "responding");
    match r {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(ConnectionError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(ConnectionError::ConnectionDropped(Some(reason))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, reason).into_response()
        }
        Err(ConnectionError::ConnectionDropped(None)) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
