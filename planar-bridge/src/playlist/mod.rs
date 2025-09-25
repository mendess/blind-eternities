use crate::Backend;
use askama::Template;
use axum::{Router, extract::Path, response::IntoResponse, routing::get};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::io;

pub fn routes() -> Router<Backend> {
    Router::new()
        .route("/", get(index))
        .route("/playlist", get(playlist))
        .route("/audio/{id}", get(audio))
    // .route("/thumb/{id}", get(thumb))
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("api request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("render error: {0}")]
    TemplateRender(#[from] askama::Error),
}

impl Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Reqwest(e) => e.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Self::TemplateRender(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

#[derive(Template)]
#[template(path = "playlist/index.html")]
struct Index {}

async fn index() -> Result<impl IntoResponse, Error> {
    Ok(Index {}.render()?)
}

#[derive(Template)]
#[template(path = "playlist/playlist.html")]
struct Playlist {
    songs: Vec<Song>,
}

#[derive(Serialize, Deserialize)]
struct Song {
    id: String,
    title: String,
    artist: Option<String>,
    categories: Vec<String>,
}

async fn playlist() -> Result<impl IntoResponse, Error> {
    let playlist = load_playlist().await.unwrap();
    Ok(Playlist {
        songs: playlist
            .songs
            .into_iter()
            .map(|s| Song {
                id: s.link.id().to_string(),
                categories: s
                    .all_categories()
                    .filter(|c| Some(*c) != s.artist.as_deref())
                    .map(|s| s.to_string())
                    .collect(),
                title: s.name,
                artist: s.artist,
            })
            .rev()
            .collect(),
    }
    .render()?)
}

#[derive(Deserialize)]
struct AudioQuery {
    id: String,
}

async fn audio(query: Path<AudioQuery>) -> Result<impl IntoResponse, Error> {
    let response = reqwest::Client::new()
        .get(format!(
            "https://mendess.xyz/api/v1/playlist/audio/{}",
            query.0.id
        ))
        .send()
        .await?
        .error_for_status()?;

    Ok((
        response.status(),
        response.headers().clone(),
        response.bytes().await?,
    ))
}

// TODO: dedup
pub async fn load_playlist() -> io::Result<mlib::playlist::Playlist> {
    // const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

    async fn init() -> io::Result<mlib::playlist::Playlist> {
        let playlist_request = reqwest::get(
            "https://raw.githubusercontent.com/mendess/spell-book/master/runes/m/playlist.json",
        )
        .await
        .map_err(io::Error::other)?;

        let text = playlist_request.text().await.map_err(io::Error::other)?;

        mlib::playlist::Playlist::load_from_str(&text).map_err(io::Error::other)
    }
    init().await
}
