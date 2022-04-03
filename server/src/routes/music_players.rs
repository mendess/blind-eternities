use actix_web::{http::StatusCode, web, HttpResponse, ResponseError};
use anyhow::Context;
use common::domain::{music::Player, Hostname};
use sqlx::PgPool;
use tracing::instrument;

pub fn routes() -> actix_web::Scope {
    web::scope("/music").service(
        web::scope("/player")
            .service(
                web::scope("/{machine}/{index}")
                    .service(
                        web::resource("")
                            .route(web::patch().to(reprioritize))
                            .route(web::post().to(new_player))
                            .route(web::delete().to(delete)),
                    )
                    .service(
                        web::resource("/last")
                            .route(web::get().to(get_last_queue))
                            .route(web::delete().to(reset_last_queue))
                            .route(web::post().to(set_last_queue)),
                    ),
            )
            .route("", web::get().to(index)),
    )
}

#[derive(Debug, thiserror::Error)]
pub enum MusicPlayersError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    #[error("duplicate player")]
    DuplicatePlayer,
    #[error("not found")]
    NotFound,
}

impl ResponseError for MusicPlayersError {
    fn status_code(&self) -> StatusCode {
        use MusicPlayersError::*;
        match self {
            UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            DuplicatePlayer => StatusCode::BAD_REQUEST,
            NotFound => StatusCode::NOT_FOUND,
        }
    }
}

impl From<sqlx::Error> for MusicPlayersError {
    fn from(e: sqlx::Error) -> Self {
        tracing::debug!(?e, "converting error");
        match e {
            sqlx::Error::Database(e) if e.code().as_deref() == Some("23505") => {
                MusicPlayersError::DuplicatePlayer
            }
            e => MusicPlayersError::UnexpectedError(e.into()),
        }
    }
}

#[instrument(name = "list players", skip(conn))]
pub async fn index(conn: web::Data<PgPool>) -> Result<HttpResponse, MusicPlayersError> {
    let mut players = sqlx::query!("SELECT * FROM music_player")
        .fetch_all(&**conn)
        .await
        .context("fetching music players")?;

    players.sort_by_key(|r| r.priority);

    let players = players
        .into_iter()
        .map(|r| {
            u8::try_from(r.player)
                .context("invalid player number in database")
                .and_then(|player| {
                    Ok(Player {
                        hostname: Hostname::try_from(r.hostname)
                            .context("invalid hostname in database")?,
                        player,
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .context("negative priorities found")?;

    Ok(HttpResponse::Ok().json(players))
}

#[instrument(name = "reprioritize a players", skip(conn))]
pub async fn reprioritize(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
) -> Result<HttpResponse, MusicPlayersError> {
    sqlx::query!(
        "UPDATE music_player SET priority=DEFAULT WHERE hostname = $1 AND player = $2",
        path.hostname.as_ref(),
        i32::from(path.player),
    )
    .execute(&**conn)
    .await?;

    Ok(HttpResponse::Ok().finish())
}

#[instrument(name = "create a new a player", skip(conn))]
pub async fn new_player(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
) -> Result<HttpResponse, MusicPlayersError> {
    sqlx::query!(
        "INSERT INTO music_player (hostname, player) VALUES ($1, $2)",
        path.hostname.as_ref(),
        i32::from(path.player),
    )
    .execute(&**conn)
    .await?;

    Ok(HttpResponse::Created().finish())
}

#[instrument(name = "default player", skip(conn))]
pub async fn current(conn: web::Data<PgPool>) -> Result<HttpResponse, MusicPlayersError> {
    let result = sqlx::query!(
        r#"SELECT hostname, player FROM music_player
        WHERE priority = (SELECT MAX(priority) FROM music_player)"#
    )
    .fetch_one(&**conn)
    .await;
    tracing::debug!(?result, "got the current player");
    let current = result.context("failed to find a player")?;

    Ok(HttpResponse::Ok().json(Player {
        hostname: Hostname::try_from(current.hostname).context("invalid hostname in database")?,
        player: current.player.try_into().context("monkas")?,
    }))
}

#[instrument(name = "delete player", skip(conn))]
pub async fn delete(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
) -> Result<HttpResponse, MusicPlayersError> {
    let result = sqlx::query!(
        "DELETE FROM music_player WHERE hostname = $1 AND player = $2",
        path.hostname.as_ref(),
        i32::from(path.player),
    )
    .execute(&**conn)
    .await?;

    if result.rows_affected() == 0 {
        Err(MusicPlayersError::NotFound)
    } else {
        Ok(HttpResponse::Ok().finish())
    }
}

#[instrument(name = "last queue get", skip(conn))]
pub async fn get_last_queue(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
) -> Result<HttpResponse, MusicPlayersError> {
    todo!()
}

#[instrument(name = "last queue set", skip(conn))]
pub async fn set_last_queue(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
    new: web::Json<usize>,
) -> Result<HttpResponse, MusicPlayersError> {
    todo!()
}

#[instrument(name = "last queue reset", skip(conn))]
pub async fn reset_last_queue(
    conn: web::Data<PgPool>,
    path: web::Path<Player>,
) -> Result<HttpResponse, MusicPlayersError> {
    todo!()
}
