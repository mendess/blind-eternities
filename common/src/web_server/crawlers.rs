use axum::{
    extract::Request,
    middleware::{self, FromFnLayer, Next},
    response::Response,
    routing::MethodRouter,
};
use http::header;
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tower::Service;

pub fn robots_txt<S: Clone + Send + Sync + 'static>() -> MethodRouter<S> {
    axum::routing::get(async || "User-agent: *\nDisallow: /\n")
}

type NoIndexFn = fn(Request, Next) -> NoIndexFuture;

pub fn no_index() -> FromFnLayer<NoIndexFn, (), (Request,)> {
    middleware::from_fn(|req, mut next| NoIndexFuture {
        future: next.call(req),
    })
}

pub struct NoIndexFuture {
    future: <Next as Service<Request>>::Future,
}

impl Future for NoIndexFuture {
    type Output = Result<Response, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let future = Pin::new(&mut self.get_mut().future);
        let Ok(mut response) = ready!(future.poll(cx));
        response.headers_mut().insert(
            const { header::HeaderName::from_static("x-robots-tag") },
            const { header::HeaderValue::from_static("noindex") },
        );
        Poll::Ready(Ok(response))
    }
}
