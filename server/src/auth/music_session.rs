use std::{
    fmt,
    future::{ready, Future},
    ops::ControlFlow,
    str::FromStr,
};

use common::domain::Hostname;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MusicSession(String);

impl fmt::Display for MusicSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

async fn handle_constraint_violations<'f, F, H, FFut, HFut, T>(
    query: F,
    try_handle_constraint: H,
) -> sqlx::Result<T>
where
    F: Fn() -> FFut,
    FFut: Future<Output = sqlx::Result<T>>,
    H: Fn(Constraint) -> HFut,
    HFut: Future<Output = sqlx::Result<ControlFlow<T>>>,
{
    loop {
        let result = query().await;
        match result {
            Err(sqlx::Error::Database(error)) => {
                match error.constraint().and_then(|c| c.parse().ok()) {
                    Some(constraint) => {
                        match try_handle_constraint(constraint).await? {
                            ControlFlow::Continue(()) => continue,
                            ControlFlow::Break(fallback) => return Ok(fallback),
                        };
                    }
                    None => break Err(sqlx::Error::Database(error)),
                }
            }
            result => break result,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Constraint {
    UniqueId,
    UniqueHostname,
}

impl FromStr for Constraint {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const UNIQUE_ID_STR: &str = "music_session_unique_ids";
        const UNIQUE_HOSTNAME_STR: &str = "music_sessions_unique_hostnames";
        match s {
            UNIQUE_ID_STR => Ok(Self::UniqueId),
            UNIQUE_HOSTNAME_STR => Ok(Self::UniqueHostname),
            _ => Err(()),
        }
    }
}

impl MusicSession {
    pub async fn create_for_2(db: &PgPool, hostname: &Hostname) -> sqlx::Result<Self> {
        let id = handle_constraint_violations(
            || {
                sqlx::query_scalar!(
                    "INSERT INTO music_sessions (id, expires_at, hostname) VALUES
                    (substr(md5(random()::text), 0, 7), NOW() + interval '4 hours', $1)
                    RETURNING id",
                    hostname.as_ref()
                )
                .fetch_one(db)
            },
            |constraint| async move {
                match constraint {
                    Constraint::UniqueId => Ok(ControlFlow::Continue(())),
                    Constraint::UniqueHostname => update_existing_token(db, hostname).await,
                }
            },
        )
        .await?;

        async fn update_existing_token(
            db: &PgPool,
            hostname: &Hostname,
        ) -> sqlx::Result<ControlFlow<String>> {
            let updated_id = sqlx::query_scalar!(
                "UPDATE music_sessions
                SET expires_at = (NOW() + interval '4 hours')
                WHERE hostname = $1 AND expires_at > NOW()
                RETURNING id",
                hostname.as_ref()
            )
            .fetch_optional(db)
            .await?;
            Ok(ControlFlow::Break(match updated_id {
                Some(id) => id,
                // existing token has expired
                None => overwrite_with_new_token(db, hostname).await?,
            }))
        }

        async fn overwrite_with_new_token(
            db: &PgPool,
            hostname: &Hostname,
        ) -> sqlx::Result<String> {
            handle_constraint_violations(
                || {
                    sqlx::query_scalar!(
                        "UPDATE music_sessions
                        SET
                            expires_at = NOW() + interval '4 hours',
                            id = substr(md5(random()::text), 0, 7)
                        WHERE hostname = $1
                        RETURNING id",
                        hostname.as_ref(),
                    )
                    .fetch_one(db)
                },
                |constraint| match constraint {
                    Constraint::UniqueId => ready(Ok(ControlFlow::Continue(()))),
                    Constraint::UniqueHostname => {
                        unreachable!("not inserting a row with a hostname")
                    }
                },
            )
            .await
        }

        Ok(Self(id))
    }

    pub async fn create_for(db: &PgPool, hostname: &Hostname) -> sqlx::Result<Self> {
        let id = loop {
            let result = sqlx::query!(
                "INSERT INTO music_sessions (id, expires_at, hostname) VALUES
                (substr(md5(random()::text), 0, 7), NOW() + interval '4 hours', $1)
                RETURNING id",
                hostname.as_ref()
            )
            .fetch_one(db)
            .await;

            match result {
                Ok(record) => break record.id,
                Err(sqlx::Error::Database(error)) => match error.constraint() {
                    Some("music_session_unique_ids") => continue,
                    Some("music_sessions_unique_hostnames") => {
                        let updated_id = sqlx::query_scalar!(
                            "UPDATE music_sessions
                            SET expires_at = (NOW() + interval '4 hours')
                            WHERE hostname = $1 AND expires_at > NOW()
                            RETURNING id",
                            hostname.as_ref()
                        )
                        .fetch_optional(db)
                        .await?;
                        match updated_id {
                            Some(id) => {
                                break id;
                            }
                            None => {
                                break loop {
                                    let result = sqlx::query_scalar!(
                                        "UPDATE music_sessions
                                        SET
                                            expires_at = NOW() + interval '4 hours',
                                            id = substr(md5(random()::text), 0, 7)
                                        WHERE hostname = $1
                                        RETURNING id",
                                        hostname.as_ref(),
                                    )
                                    .fetch_one(db)
                                    .await;
                                    match result {
                                        Ok(id) => break id,
                                        Err(sqlx::Error::Database(error)) => {
                                            match error.constraint() {
                                                Some("music_session_unique_ids") => continue,
                                                _ => return Err(sqlx::Error::Database(error)),
                                            }
                                        }
                                        Err(e) => return Err(e),
                                    }
                                };
                            }
                        }
                    }
                    _ => return Err(sqlx::Error::Database(error)),
                },
                Err(e) => return Err(e),
            }
        };

        Ok(Self(id))
    }

    pub async fn hostname(&self, db: &PgPool) -> sqlx::Result<Option<Hostname>> {
        Ok(sqlx::query!(
            "SELECT hostname FROM music_sessions WHERE id = $1 AND expires_at > NOW()",
            &self.0
        )
        .fetch_optional(db)
        .await?
        .map(|r| Hostname::try_from(r.hostname).unwrap()))
    }

    pub async fn delete(self, db: &PgPool) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM music_sessions WHERE id = $1", self.0.as_str())
            .execute(db)
            .await?;
        Ok(())
    }
}
