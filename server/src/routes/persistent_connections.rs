use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use common::domain::Hostname;
use http::StatusCode;

use crate::{
    auth,
    persistent_connections::{ConnectionError, Connections},
};

pub fn routes() -> Router<super::RouterState> {
    Router::new()
        .route("/", get(list_persistent_connections))
        .route("/send/:hostname", post(send))
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
