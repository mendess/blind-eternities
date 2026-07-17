mod bg3;
mod minecraft;

use crate::{RouterState, util};
use askama::Template;
use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Config {
    minecraft: minecraft::Config,
}

pub fn router(config: Config) -> Router<RouterState> {
    Router::new()
        .merge(util::append_slash_router(&["/bg3", "/minecraft"]))
        .route("/", get(index))
        .nest("/bg3/", bg3::router())
        .nest("/minecraft/", minecraft::router(Arc::new(config.minecraft)))
}

async fn index() -> impl IntoResponse {
    #[derive(Template)]
    #[template(path = "games/index.html")]
    struct Index;

    Html(Index.render().unwrap())
}
