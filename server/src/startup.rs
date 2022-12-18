use std::net::TcpListener;

use actix_web::{dev::Server, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use sqlx::PgPool;
use tracing_actix_web::TracingLogger;

use crate::routes::*;

pub fn run(
    listener: TcpListener,
    connection: PgPool,
    allow_any_localhost_token: bool,
) -> std::io::Result<Server> {
    let conn = web::Data::new(connection);
    let bearer_auth = HttpAuthentication::bearer(move |r, b| {
        crate::auth::verify_token(r, b, allow_any_localhost_token)
    });
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(bearer_auth.clone())
            .route("/health_check", web::get().to(health_check))
            .service(machine_status::routes())
            .app_data(conn.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
