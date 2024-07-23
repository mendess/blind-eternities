use std::{
    future::{Future, IntoFuture},
    io,
    sync::OnceLock,
};

use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    routing::get,
    Router,
};
use http::Method;
use prometheus::{register_int_counter_vec, Encoder, IntCounterVec};
use tokio::net::TcpListener;

pub fn new_request(route: &str, method: &Method) {
    static METRICS: OnceLock<IntCounterVec> = OnceLock::new();

    METRICS
        .get_or_init(|| {
            register_int_counter_vec!(
                "requests",
                "number of requests by route",
                &["route", "method"]
            )
            .unwrap()
        })
        .with_label_values(&[route, method.as_str()])
        .inc();
}

pub async fn metrics_handler(prefix: &str) -> String {
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    let mut metrics = prometheus::gather();
    for m in &mut metrics {
        let name = format!("{prefix}_{}", m.get_name());
        m.set_name(name);
    }
    if let Err(e) = encoder.encode(&metrics, &mut buffer) {
        tracing::error!(error = ?e, "could not encode custom metrics");
    }

    match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = ?e, "custom metrics aren't valid utf8");
            String::default()
        }
    }
}

pub async fn start_metrics_endpoint(
    prefix: &'static str,
) -> io::Result<impl Future<Output = io::Result<()>>> {
    Ok(axum::serve(
        TcpListener::bind("0.0.0.0:9000").await?,
        Router::new().route("/metrics", get(|| metrics_handler(prefix))),
    )
    .into_future())
}

#[derive(Clone, Copy, Default, Debug)]
pub struct RequestMetrics;

impl RequestMetrics {
    pub async fn as_fn(matched: MatchedPath, req: Request, next: Next) -> axum::response::Response {
        new_request(matched.as_str(), req.method());
        next.run(req).await
    }
}
