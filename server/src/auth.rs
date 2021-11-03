use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use actix_web::{dev::ServiceRequest, http::StatusCode, web, ResponseError};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use anyhow::Context;
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
    allow_any_localhost_token: bool,
) -> Result<ServiceRequest, actix_web::Error> {
    let conn = req
        .app_data::<web::Data<PgPool>>()
        .expect("pg pool not configured");

    fn is_localhost(addr: SocketAddr) -> bool {
        match addr.ip() {
            IpAddr::V4(ip) => ip == Ipv4Addr::LOCALHOST,
            IpAddr::V6(ip) => ip == Ipv6Addr::LOCALHOST,
        }
    }

    if allow_any_localhost_token && matches!(req.peer_addr(), Some(ip) if is_localhost(ip)) {
        return Ok(req);
    }

    match sqlx::query!(
        "SELECT token FROM api_tokens WHERE token = $1",
        uuid::Uuid::parse_str(bearer.token()).map_err(|_| AuthError::InvalidToken)?
    )
    .fetch_optional(conn.get_ref())
    .await
    .context("failed to fetch token from db")
    .map_err(AuthError::UnexpectedError)?
    {
        Some(_) => Ok(req),
        None => Err(AuthError::UnauthorizedToken.into()),
    }
}
