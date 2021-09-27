use std::net::TcpListener;

use actix_web::{dev::Server, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use sqlx::PgPool;
use tracing_actix_web::TracingLogger;

use crate::routes::*;

pub fn run(listener: TcpListener, connection: PgPool) -> std::io::Result<Server> {
    let conn = web::Data::new(connection);
    let auth = HttpAuthentication::bearer(crate::auth::verify_token);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(auth.clone())
            .route("/health_check", web::get().to(health_check))
            .service(
                web::resource("/machine/status")
                    .route(web::post().to(machine_status::post))
                    .route(web::get().to(machine_status::get))
            )
            .app_data(conn.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
