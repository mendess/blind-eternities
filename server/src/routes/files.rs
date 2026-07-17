use crate::{
    routes::{RouterState, dirs::Directory as _},
    util,
};
use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use http::StatusCode;
use std::{io, path::Path};
use tokio_stream::StreamExt as _;

pub fn routes() -> Router<RouterState> {
    Router::new()
        .route("/", get(index))
        .route("/{filename}", get(load_file::<false>))
        .route("/unlisted/{filename}", get(load_file::<true>))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("file not found")]
    NotFound,
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotFound => StatusCode::NOT_FOUND,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

pub async fn index(state: State<RouterState>) -> Result<impl IntoResponse, Error> {
    Ok(Json(
        util::fs::list_files_at(state.dirs.files().get())
            .await?
            .filter(|p| p != Path::new("unlisted"))
            .collect::<Vec<_>>()
            .await,
    ))
}

pub async fn load_file<const UNLISTED: bool>(
    state: State<RouterState>,
    filename: axum::extract::Path<String>,
) -> Result<impl IntoResponse, Error> {
    let file_path = if UNLISTED {
        state.dirs.files().unlisted().file(&filename)
    } else {
        state.dirs.files().file(&filename)
    };

    match util::fs::named_file(&file_path).await {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::NotFound),
        Err(e) => Err(e.into()),
    }
}
