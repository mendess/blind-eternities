use std::sync::Arc;

use axum::{extract::FromRef, Router};
use sqlx::PgPool;

use crate::persistent_connections::Connections;

pub mod admin;
pub mod machine_status;
pub mod music;
pub mod persistent_connections;

#[derive(Debug, Clone)]
pub struct RouterState {
    pub connections: Arc<Connections>,
    pub db: Arc<PgPool>,
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

pub fn router(connections: Arc<Connections>, db: Arc<PgPool>) -> Router {
    Router::new()
        .nest("/admin", admin::routes())
        .nest("/machine", machine_status::routes())
        .nest("/persistent-connections", persistent_connections::routes())
        .nest("/music", music::routes())
        .with_state(RouterState { connections, db })
}
