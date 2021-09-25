use std::net::TcpListener;

use actix_web::{dev::Server, web, App, HttpServer};
use sqlx::PgPool;
use tracing_actix_web::TracingLogger;

use crate::routes::*;

pub fn run(listener: TcpListener, connection: PgPool) -> std::io::Result<Server> {
    let conn = web::Data::new(connection);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            .route("/machine/status", web::post().to(machine_status))
            .app_data(conn.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
