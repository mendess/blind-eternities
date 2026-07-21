use crate::{RouterState, util};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::{Html, IntoResponse},
    routing::get,
};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::io;

pub fn routes() -> Router<RouterState> {
    let make_router = |dir: &'static str| -> Router<RouterState> {
        Router::new()
            .route(
                "/",
                get(async move |state, query| index(dir, state, query).await),
            )
            .route("/random", get(random))
            .route(
                "/random-file",
                get(async move |state| proxy_wallpaper(dir, state, None).await),
            )
            .route(
                "/thumb/{filename}",
                get(async move |state, path| thumb(dir, state, path).await),
            )
            .route(
                "/{filename}",
                get(async move |state: State<RouterState>, path| {
                    proxy_wallpaper(dir, state, Some(path)).await
                }),
            )
    };
    Router::new()
        .merge(util::append_slash_router(&["/all", "/phone"]))
        .merge(make_router("walls/small"))
        .route(
            "/small/{filename}",
            get(async move |state, path| proxy_wallpaper("walls/small", state, Some(path)).await),
        )
        .nest("/all/", make_router("walls/all"))
        .nest("/phone/", make_router("walls/phone"))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("invalid url: {0}")]
    InvalidUrl(#[from] url::ParseError),
    // #[error("bad request: {0}")]
    // BadRequest(String),
    #[error("render error: {0}")]
    TemplateRender(#[from] askama::Error),
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) | Self::Reqwest(_) | Self::TemplateRender(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            // Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidUrl(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

#[derive(Deserialize)]
struct Wallpaper {
    path: String,
    #[serde(default)]
    thumb: String,
    name: String,
}

#[derive(Template)]
#[template(path = "walls/index.html")]
struct Index {
    walls: Vec<Wallpaper>,
}

#[derive(Serialize, Deserialize)]
struct IndexQuery {
    #[serde(default)]
    mtg: bool,
}

async fn index(
    dir: &'static str,
    state: State<RouterState>,
    query: Query<IndexQuery>,
) -> Result<impl IntoResponse, Error> {
    Ok(Html(
        Index {
            walls: state
                .client
                .get(dir.trim_end_matches('/'))
                .unwrap()
                .query(&query.0)
                .send()
                .await?
                .json::<Vec<Wallpaper>>()
                .await?
                .into_iter()
                .map(|w| Wallpaper {
                    thumb: format!("thumb/{}", w.path),
                    ..w
                })
                .collect(),
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "walls/wallpaper.html")]
struct Random {
    name: Option<String>,
    url: String,
}

async fn random() -> Result<impl IntoResponse, Error> {
    // this guarantees the browser doesn't cache that random output
    let time_string = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    Ok(Html(
        Random {
            name: None,
            url: format!("./random-file?v={}", time_string.as_millis()),
        }
        .render()?,
    ))
}

async fn proxy_wallpaper(
    dir: &'static str,
    state: State<RouterState>,
    filename: Option<Path<String>>,
) -> Result<impl IntoResponse, Error> {
    let filename = filename
        .as_ref()
        .map(|Path(path)| path.as_str())
        .unwrap_or("random");
    let Ok(filename) = askama::filters::urlencode(filename);
    Ok(common::web_server::reqwest_to_axum(
        state
            .client
            .get(&format!("{dir}/{filename}"))
            .unwrap()
            .send()
            .await?,
    )?)
}

async fn thumb(
    dir: &'static str,
    state: State<RouterState>,
    Path(filename): Path<String>,
) -> Result<impl IntoResponse, Error> {
    let Ok(filename) = askama::filters::urlencode(filename);
    Ok(common::web_server::reqwest_to_axum(
        state
            .client
            .get(&format!("{dir}/thumb/{filename}"))
            .unwrap()
            .send()
            .await?,
    )?)
}
