use actix_web::{dev::ServiceRequest, http::StatusCode, web, ResponseError};
use actix_web_httpauth::{
    extractors::{
        bearer::{BearerAuth, Config},
        AuthenticationError,
    },
    headers::www_authenticate::bearer::Bearer,
};
use anyhow::Context;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use sqlx::PgPool;

#[derive(thiserror::Error, Debug)]
enum AuthError {
    #[error("Invalid token")]
    InvalidToken,
    #[error("Unauthorized token")]
    UnauthorizedToken,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for AuthError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidToken => StatusCode::BAD_REQUEST,
            Self::UnauthorizedToken => StatusCode::UNAUTHORIZED,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub async fn verify_token(
    req: ServiceRequest,
    bearer: BearerAuth,
) -> Result<ServiceRequest, actix_web::Error> {
    let config = req.app_data::<Config>().cloned().unwrap_or_default();
    let conn = req
        .app_data::<web::Data<PgPool>>()
        .expect("pg pool not configured");

    match sqlx::query!(
        "SELECT token FROM api_tokens WHERE token = $1",
        uuid::Uuid::parse_str(bearer.token()).map_err(|_| AuthError::InvalidToken)?
    )
    .fetch_optional(&***conn)
    .await
    .context("failed to fetch token from db")
    .map_err(AuthError::UnexpectedError)?
    {
        Some(token) => Ok(req),
        None => Err(AuthError::UnauthorizedToken.into()),
    }
}
