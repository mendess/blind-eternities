use crate::routes;
use axum::middleware::from_fn;
use common::telemetry::metrics::RequestMetrics;
use sqlx::PgPool;
use std::{
    future::{self, Future, IntoFuture},
    io,
    sync::Arc,
};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

pub fn run(
    server_listener: TcpListener,
    persistent_conns_listener: TcpListener,
    db: PgPool,
) -> io::Result<impl Future<Output = io::Result<()>>> {
    let db = Arc::new(db);
    let connections = crate::persistent_connections::start_persistent_connections_daemon(
        persistent_conns_listener,
        db.clone(),
    );

    Ok(axum::serve(
        server_listener,
        routes::router(connections, db)
            .layer(from_fn(RequestMetrics::as_fn))
            .layer(TraceLayer::new_for_http()),
    )
    .with_graceful_shutdown(async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = ?e, "failed to setup shutdown signal");
            future::pending().await
        }
    })
    .into_future())
}
