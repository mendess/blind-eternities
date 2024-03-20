pub mod connections;
mod daemon;

use sqlx::PgPool;
use std::sync::Arc;
use tokio::net::TcpListener;

pub use connections::{ConnectionError, Connections};

pub fn start_persistent_connections_daemon(
    listener: TcpListener,
    db: Arc<PgPool>,
) -> Arc<Connections> {
    let connections = Arc::new(Connections::new());
    tokio::spawn(daemon::start(listener, connections.clone(), db));
    connections
}
