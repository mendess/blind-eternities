use actix_web::{web, HttpResponse, Responder, ResponseError};
use common::domain::{music_session::ExpiresAt, Hostname};
use sqlx::PgPool;

use crate::auth::{self, music_session::MusicSession};

pub fn routes() -> actix_web::Scope {
    web::scope("/admin")
        .service(web::resource("/health_check").route(web::get().to(health_check)))
        .service(
            web::resource("/music-session/{hostname}")
                .route(web::get().to(create_music_session))
                .route(web::delete().to(delete_music_session)),
        )
}

async fn health_check(_: auth::Admin) -> impl Responder {
    HttpResponse::Ok()
}

#[derive(thiserror::Error, Debug)]
pub enum MusicSessionError {
    #[error(transparent)]
    AuthError(#[from] auth::AuthError),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
}

impl ResponseError for MusicSessionError {}

#[tracing::instrument(skip(db))]
async fn create_music_session(
    _: auth::Admin,
    db: web::Data<PgPool>,
    hostname: web::Path<Hostname>,
    web::Query(ExpiresAt { expires_at }): web::Query<ExpiresAt>,
) -> Result<impl Responder, MusicSessionError> {
    let id = MusicSession::create_for(&db, &hostname, expires_at).await?;
    tracing::info!("created id = {id}");

    Ok(HttpResponse::Ok().json(id))
}

#[tracing::instrument(skip(db))]
async fn delete_music_session(
    _: auth::Admin,
    db: web::Data<PgPool>,
    id: web::Path<MusicSession>,
) -> Result<impl Responder, MusicSessionError> {
    id.into_inner().delete(&db).await?;
    Ok(HttpResponse::Ok())
}
