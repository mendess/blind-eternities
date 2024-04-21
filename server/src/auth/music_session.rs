use std::fmt;

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

impl MusicSession {
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
                        break sqlx::query!(
                            "UPDATE music_sessions
                            SET expires_at = (NOW() + interval '4 hours')
                            WHERE hostname = $1
                            RETURNING id",
                            hostname.as_ref()
                        )
                        .fetch_one(db)
                        .await?
                        .id;
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
