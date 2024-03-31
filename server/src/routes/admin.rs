use actix_web::{web, HttpResponse, Responder, ResponseError};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth, persistent_connections::Connections};

pub fn routes() -> actix_web::Scope {
    web::scope("/admin")
        .service(web::resource("/health_check").route(web::get().to(health_check)))
        .service(
            web::resource("/music_token")
                .route(web::post().to(add_music_token))
                .route(web::delete().to(delete_music_token)),
        )
        .route(
            "/persistent-connections",
            web::get().to(list_persistent_connections),
        )
}

async fn health_check(_: auth::Admin) -> impl Responder {
    HttpResponse::Ok()
}

#[derive(thiserror::Error, Debug)]
pub enum MusicTokenError {
    #[error(transparent)]
    AuthError(#[from] auth::AuthError),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
}

impl ResponseError for MusicTokenError {}

async fn add_music_token(
    _: auth::Admin,
    db: web::Data<PgPool>,
    username: String,
) -> Result<HttpResponse, MusicTokenError> {
    let new_token = Uuid::new_v4();
    auth::insert_token::<auth::Music>(&db, new_token, &username).await?;
    Ok(HttpResponse::Ok().json(new_token))
}

async fn delete_music_token(
    _: auth::Admin,
    db: web::Data<PgPool>,
    username: String,
) -> Result<HttpResponse, MusicTokenError> {
    auth::delete_token::<auth::Music>(&db, &username).await?;
    Ok(HttpResponse::Ok().into())
}

async fn list_persistent_connections(connections: web::Data<Connections>) -> impl Responder {
    let connected = connections.connected_hosts().await;
    HttpResponse::Ok().json(connected.into_iter().map(|(h, _)| h).collect::<Vec<_>>())
}
