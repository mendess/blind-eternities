use axum::{
    Router,
    body::Body,
    response::{Redirect, Response},
    routing::get,
};
use std::io;

pub fn proxy_response(mut response: reqwest::Response) -> io::Result<Response> {
    let mut response_builder = Response::builder().status(response.status());
    *response_builder.headers_mut().unwrap() = std::mem::take(response.headers_mut());
    response_builder
        .body(Body::from_stream(response.bytes_stream()))
        .map_err(io::Error::other)
}

pub fn append_slash_router<S>(routes: &[&'static str]) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut router = Router::new();
    for r in routes {
        router = router.route(r, get(Redirect::to(&format!(".{r}/"))));
    }
    router
}
