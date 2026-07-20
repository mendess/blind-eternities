use crate::RouterState;
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::{Html, IntoResponse},
    routing::get,
};
use http::StatusCode;
use std::io;

pub fn routes() -> Router<RouterState> {
    Router::new()
        .route("/", get(index))
        .route("/{filename}", get(proxy_file::<false>))
        .route("/unlisted/{filename}", get(proxy_file::<true>))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("render error: {0}")]
    TemplateRender(#[from] askama::Error),
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) | Self::TemplateRender(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Reqwest(e) => e.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

#[derive(Template)]
#[template(path = "files.html")]
struct Index {
    files: Vec<String>,
}

pub async fn index(state: State<RouterState>) -> Result<impl IntoResponse, Error> {
    Ok(Html(
        Index {
            files: state
                .client
                .get("/files")
                .unwrap()
                .send()
                .await?
                .json()
                .await?,
        }
        .render()?,
    ))
}

pub async fn proxy_file<const UNLISTED: bool>(
    state: State<RouterState>,
    Path(filename): Path<String>,
) -> Result<impl IntoResponse, Error> {
    let path = if UNLISTED {
        format!("/files/unlisted/{filename}")
    } else {
        format!("/files/{filename}")
    };
    Ok(common::net::proxy::reqwest_to_axum(
        state.client.get(&path).unwrap().send().await?,
    )?)
}
