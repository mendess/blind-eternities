use std::{future::Future, io, net::TcpListener, sync::OnceLock};

use actix_http::Method;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use futures::TryFutureExt;
use prometheus::{register_int_counter_vec, Encoder, IntCounterVec};

pub fn new_request(route: &str, method: &Method) {
    static METRICS: OnceLock<IntCounterVec> = OnceLock::new();

    METRICS
        .get_or_init(|| {
            register_int_counter_vec!(
                "blind_eternities_requests",
                "number of requests by route",
                &["route", "method"]
            )
            .unwrap()
        })
        .with_label_values(&[route, method.as_str()])
        .inc();
}

async fn metrics_handler() -> impl Responder {
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        tracing::error!(error = ?e, "could not encode custom metrics");
    }

    let res = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = ?e, "custom metrics aren't valid utf8");
            String::default()
        }
    };
    HttpResponse::Ok().body(res)
}

pub fn start_metrics_endpoint() -> io::Result<impl Future<Output = io::Result<()>>> {
    Ok(
        HttpServer::new(|| App::new().route("/metrics", web::get().to(metrics_handler)))
            .listen(TcpListener::bind("0.0.0.0:9000")?)?
            .run()
            .into_future(),
    )
}
