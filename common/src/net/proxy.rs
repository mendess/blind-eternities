use axum::{body::Body, response::Response};
use std::io;

pub fn reqwest_to_axum(mut response: reqwest::Response) -> io::Result<Response> {
    let mut response_builder = Response::builder().status(response.status());
    *response_builder.headers_mut().unwrap() = std::mem::take(response.headers_mut());
    response_builder
        .body(Body::from_stream(response.bytes_stream()))
        .map_err(io::Error::other)
}
