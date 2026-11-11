mod good_lands;

use crate::RouterState;
use askama::Template;
use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};
use http::StatusCode;

pub fn router() -> Router<RouterState> {
    Router::new()
        .route("/", get(index))
        .route("/field-guide", get(field_guide))
        .nest("/lands", good_lands::router())
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("rendering: {0}")]
    Rendering(#[from] askama::Error),
    #[error("badrequest")]
    BadRequest,
    #[error("internal server error: {0}")]
    InternalServerError(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Rendering(_) | Self::InternalServerError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
            }
            Self::BadRequest => (StatusCode::BAD_REQUEST, self.to_string()).into_response(),
        }
    }
}

#[derive(Template)]
#[template(path = "games/mtg/index.html")]
struct Index;

async fn index() -> Result<impl IntoResponse, Error> {
    Ok(Html(Index.render()?))
}

#[derive(Template)]
#[template(path = "games/mtg/field-guide.html")]
struct FieldGuide;

async fn field_guide() -> Result<impl IntoResponse, Error> {
    Ok(Html(FieldGuide.render()?))
}
