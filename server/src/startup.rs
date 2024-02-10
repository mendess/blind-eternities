use std::{net as std_net, sync::Arc};

use actix_service::Service;
use actix_web::{dev::Server, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use sqlx::PgPool;
use tokio::net as tokio_net;
use tracing_actix_web::TracingLogger;

use crate::{metrics, routes::*};

pub struct RunConfig {
    pub allow_any_localhost_token: bool,
    pub override_num_workers: Option<usize>,
}

pub fn run(
    server_listener: std_net::TcpListener,
    persistent_conns_listener: tokio_net::TcpListener,
    db: PgPool,
    run_config: RunConfig,
) -> std::io::Result<Server> {
    let db = Arc::new(db);
    let allow_any_localhost_token = run_config.allow_any_localhost_token;
    let bearer_auth = HttpAuthentication::bearer(move |r, b| {
        crate::auth::verify_token(r, b, allow_any_localhost_token)
    });
    let connections = web::Data::from(
        crate::persistent_connections::start_persistent_connections_daemon(
            persistent_conns_listener,
            db.clone(),
            allow_any_localhost_token,
        ),
    );
    tokio::spawn(metrics::start_metrics_endpoint()?);
    let conn = web::Data::from(db);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(bearer_auth.clone())
            .wrap_fn(|req, srv| {
                tracing::info!(req_name = ?req.match_name(), "request received");
                metrics::new_request(
                    req.match_pattern().as_deref().unwrap_or("UNMATCHED"),
                    req.method(),
                );
                srv.call(req)
            })
            .route("/health_check", web::get().to(health_check))
            .service(machine_status::routes())
            .service(remote_spark::routes())
            .service(music_players::routes())
            .service(persistent_connections::routes())
            .app_data(conn.clone())
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
