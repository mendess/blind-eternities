use std::{net as std_net, sync::Arc};

use actix_web::{dev::Server, web, App, HttpServer};
use sqlx::PgPool;
use tokio::net as tokio_net;
use tracing_actix_web::TracingLogger;

use crate::routes::*;

pub struct RunConfig {
    pub override_num_workers: Option<usize>,
    pub enable_metrics: bool,
}

pub fn run(
    server_listener: std_net::TcpListener,
    persistent_conns_listener: tokio_net::TcpListener,
    db: PgPool,
    run_config: RunConfig,
) -> std::io::Result<Server> {
    let db = Arc::new(db);
    let connections = web::Data::from(
        crate::persistent_connections::start_persistent_connections_daemon(
            persistent_conns_listener,
            db.clone(),
        ),
    );
    if run_config.enable_metrics {
        match common::telemetry::metrics::start_metrics_endpoint("blind_eternities") {
            Ok(fut) => {
                tokio::spawn(fut);
            }
            Err(error) => {
                tracing::warn!(?error, "failed to start metrics endpoint");
            }
        }
    }
    let db = web::Data::from(db);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(common::telemetry::metrics::RequestMetrics)
            .service(admin::routes())
            .service(machine_status::routes())
            .service(persistent_connections::routes())
            .service(music::routes())
            .app_data(db.clone())
            .app_data(connections.clone())
    })
    .listen(server_listener)?;
    let server = if let Some(workers) = run_config.override_num_workers {
        server.workers(workers)
    } else {
        server
    };
    Ok(server.run())
}
