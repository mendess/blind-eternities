use std::{net as std_net, sync::Arc};

use actix_web::{dev::Server, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use sqlx::PgPool;
use tokio::net as tokio_net;
use tracing_actix_web::TracingLogger;

use crate::{persistent_connections::Connections, routes::*};

pub fn run(
    server_listener: std_net::TcpListener,
    persistent_conns_listener: tokio_net::TcpListener,
    connection: PgPool,
    allow_any_localhost_token: bool,
) -> std::io::Result<Server> {
    let conn = Arc::new(connection);
    let bearer_auth = HttpAuthentication::bearer(move |r, b| {
        crate::auth::verify_token(r, b, allow_any_localhost_token)
    });
    let connections = Arc::new(Connections::new());
    tokio::spawn(crate::persistent_connections::start(
        persistent_conns_listener,
        connections.clone(),
        conn.clone(),
        allow_any_localhost_token,
    ));
    let conn = web::Data::from(conn);
    let connections = web::Data::from(connections);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(bearer_auth.clone())
            .route("/health_check", web::get().to(health_check))
            .service(machine_status::routes())
            .service(remote_spark::routes())
            .service(music_players::routes())
            .app_data(conn.clone())
            .app_data(connections.clone())
    })
    .listen(server_listener)?
    .run();
    Ok(server)
}
