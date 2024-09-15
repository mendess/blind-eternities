use std::sync::Arc;

use axum::{extract::FromRef, Router};
use sqlx::PgPool;

use crate::persistent_connections::{ws::SocketIo, Connections};

pub mod admin;
pub mod machine_status;
pub mod music;
pub mod persistent_connections;

#[derive(Debug, Clone)]
pub struct RouterState {
    pub connections: Arc<Connections>,
    pub db: Arc<PgPool>,
    pub socket_io: SocketIo,
}

impl AsRef<Connections> for RouterState {
    fn as_ref(&self) -> &Connections {
        &self.connections
    }
}

impl AsRef<PgPool> for RouterState {
    fn as_ref(&self) -> &PgPool {
        &self.db
    }
}

impl AsRef<SocketIo> for RouterState {
    fn as_ref(&self) -> &SocketIo {
        &self.socket_io
    }
}

impl FromRef<RouterState> for Arc<Connections> {
    fn from_ref(input: &RouterState) -> Self {
        input.connections.clone()
    }
}

impl FromRef<RouterState> for Arc<PgPool> {
    fn from_ref(input: &RouterState) -> Self {
        input.db.clone()
    }
}

impl FromRef<RouterState> for SocketIo {
    fn from_ref(input: &RouterState) -> Self {
        input.socket_io.clone()
    }
}

pub fn router(connections: Arc<Connections>, db: Arc<PgPool>, socket_io: SocketIo) -> Router {
    Router::new()
        .nest("/admin", admin::routes())
        .nest("/machine", machine_status::routes())
        .nest("/persistent-connections", persistent_connections::routes())
        .nest("/music", music::routes())
        .with_state(RouterState {
            connections,
            db,
            socket_io,
        })
}
