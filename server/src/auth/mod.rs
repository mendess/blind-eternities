pub mod music_session;

use anyhow::Context as _;
use axum::{extract::FromRequestParts, http::request::Parts, response::IntoResponse};
use http::StatusCode;
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

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let code = match self {
            Self::InvalidToken => StatusCode::BAD_REQUEST,
            Self::UnauthorizedToken => StatusCode::UNAUTHORIZED,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (code, self.to_string()).into_response()
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
pub async fn check_token<R>(conn: &PgPool, token: Uuid) -> Result<R, AuthError>
where
    R: Role,
{
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
        #[axum::async_trait]
        impl<S> FromRequestParts<S> for $role
        where
            S: Send + Sync + AsRef<PgPool>,
        {
            type Rejection = AuthError;

            async fn from_request_parts(
                req: &mut Parts,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                let bearer = req
                    .headers
                    .get(axum::http::header::AUTHORIZATION)
                    .ok_or(AuthError::UnauthorizedToken)?
                    .to_str()
                    .map_err(|_| AuthError::InvalidToken)?
                    .strip_prefix("Bearer ")
                    .ok_or(AuthError::InvalidToken)?
                    .parse()
                    .map_err(|_| AuthError::InvalidToken)?;

                check_token(state.as_ref(), bearer).await
            }
        }
    };
}

gen_role_extractor!(Admin);
gen_role_extractor!(Music);
