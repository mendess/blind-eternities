use crate::routes::{self, PlaylistConfig};
use common::telemetry::metrics::MetricsEndpoint;
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
    metrics_listener: impl Into<Option<TcpListener>>,
    db: PgPool,
    playlist_config: PlaylistConfig,
) -> io::Result<impl Future<Output = io::Result<()>>> {
    let db = Arc::new(db);
    let (ws_layer, io) = crate::persistent_connections::ws::socket_io_routes(db.clone());

    let mut router = routes::router(db, io, playlist_config);

    if let Some(l) = metrics_listener.into() {
        let MetricsEndpoint { worker, layer } =
            common::telemetry::metrics::start_metrics_endpoint("blind_eternities", l);
        tokio::spawn(worker);
        router = router.layer(layer);
    }

    Ok(axum::serve(
        server_listener,
        router.layer(ws_layer).layer(TraceLayer::new_for_http()),
    )
    .with_graceful_shutdown(async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = ?e, "failed to setup shutdown signal");
            future::pending().await
        }
    })
    .into_future())
}
