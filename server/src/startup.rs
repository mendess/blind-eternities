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
            .service(
                web::scope("/machine").service(
                    web::resource("/status")
                        .route(web::get().to(machine_status::get))
                        .route(web::post().to(machine_status::post)),
                ),
            )
            .service(
                web::scope("/music").service(
                    web::scope("/players")
                        .service(
                            web::resource("")
                                .route(web::get().to(music_players::index))
                                .route(web::patch().to(music_players::reprioritize))
                                .route(web::post().to(music_players::new_player))
                                .route(web::delete().to(music_players::delete)),
                        )
                        .route("/current", web::get().to(music_players::current)),
                ),
            )
            .app_data(conn.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
