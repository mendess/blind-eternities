pub mod music_session;

use std::future::ready;

use actix_http::StatusCode;
use actix_web::{web, FromRequest, ResponseError};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use anyhow::Context as _;
use futures::{future::LocalBoxFuture, FutureExt, TryFutureExt};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
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

#[tracing::instrument(skip_all, fields(machine_name = machine, token_kind = ?R::KIND))]
pub async fn insert_token<R: Role>(pool: &PgPool, token: Uuid, machine: &str) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO api_tokens (token, created_at, hostname, role) VALUES ($1, NOW(), $2, $3)",
        token,
        machine,
        R::KIND.expect("can't insert a role for the Nobody role") as priv_role::RoleKind,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, fields(machine_name = name, token_kind = ?R::KIND))]
pub async fn delete_token<R: Role>(pool: &PgPool, name: &str) -> Result<(), AuthError> {
    let Some(role) = R::KIND else {
        return Err(AuthError::InvalidToken);
    };
    sqlx::query!(
        "DELETE FROM api_tokens WHERE hostname = $1 AND role = $2",
        name,
        role as priv_role::RoleKind
    )
    .execute(pool)
    .await
    .context("failed to delete token from db")?;
    Ok(())
}

#[async_recursion::async_recursion]
#[tracing::instrument(skip_all, fields(token_kind = ?R::KIND, result))]
pub async fn check_token<R: Role>(conn: &PgPool, token: Uuid) -> Result<R, AuthError> {
    let role = match R::KIND {
        Some(role) => role,
        None => return Err(AuthError::UnauthorizedToken),
    };
    let result = sqlx::query_scalar!(
        "SELECT hostname FROM api_tokens WHERE token = $1 AND role = $2",
        token,
        role as priv_role::RoleKind
    )
    .fetch_optional(conn)
    .await
    .context("failed to fetch token from db")?;

    if let Some(hostname) = result {
        tracing::info!(auth = hostname, "authorized");
        Ok(R::INSTANCE)
    } else {
        check_token::<R::Parent>(conn, token)
            .await
            .map(|_| R::INSTANCE)
    }
}

mod priv_role {
    #[derive(sqlx::Type, Debug)]
    #[sqlx(type_name = "role", rename_all = "lowercase")]
    pub enum RoleKind {
        Admin,
        Music,
    }

    pub trait Role: Send {
        type Parent: Role;

        /// value to send to DB
        const KIND: Option<RoleKind>;

        /// instance of self
        const INSTANCE: Self;
    }

    #[derive(Debug)]
    pub struct NoBody {}

    impl Role for NoBody {
        type Parent = Self;
        const KIND: Option<RoleKind> = None;
        const INSTANCE: Self = Self {};
    }
}

pub trait Role: priv_role::Role {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Admin {}

impl priv_role::Role for Admin {
    type Parent = priv_role::NoBody;
    const KIND: Option<priv_role::RoleKind> = Some(priv_role::RoleKind::Admin);
    const INSTANCE: Self = Self {};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Music {}

impl priv_role::Role for Music {
    type Parent = Admin;
    const KIND: Option<priv_role::RoleKind> = Some(priv_role::RoleKind::Music);
    const INSTANCE: Self = Self {};
}

impl<T> Role for T where T: priv_role::Role {}

macro_rules! gen_role_extractor {
    ($role:ident) => {
        impl FromRequest for $role {
            type Error = AuthError;
            type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;
            fn from_request(
                req: &actix_web::HttpRequest,
                payload: &mut actix_http::Payload,
            ) -> Self::Future {
                let bearer_future = BearerAuth::from_request(req, payload)
                    .map_err(|_| AuthError::UnauthorizedToken)
                    .and_then(|bearer| {
                        ready(Uuid::parse_str(bearer.token()).map_err(|_| AuthError::InvalidToken))
                    });

                let conn = req
                    .app_data::<web::Data<PgPool>>()
                    .expect("pg pool not configured")
                    .clone();

                async move { check_token::<$role>(&conn, bearer_future.await?).await }.boxed_local()
            }
        }
    };
}

gen_role_extractor!(Admin);
gen_role_extractor!(Music);
