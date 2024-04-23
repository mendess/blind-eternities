use std::{future::Future, io, net::TcpListener, sync::OnceLock};

use actix_http::Method;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use futures::TryFutureExt;
use prometheus::{register_int_counter_vec, register_int_gauge, Encoder, IntCounterVec, IntGauge};

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

pub fn live_persistent_connection_sockets() -> &'static IntGauge {
    static METRICS: OnceLock<IntGauge> = OnceLock::new();
    METRICS.get_or_init(|| {
        register_int_gauge!(
            "live_persistent_connection_sockets",
            "Number of open sockets for persistent connections",
        )
        .unwrap()
    })
}

fn persistent_connections() -> &'static IntGauge {
    static METRICS: OnceLock<IntGauge> = OnceLock::new();

    METRICS.get_or_init(|| {
        let metric = register_int_gauge!(
            "persistent_connections",
            "number of persistent_connections online"
        )
        .unwrap();
        metric.set(0);
        metric
    })
}

pub fn persistent_connected() {
    persistent_connections().inc();
}

pub fn persistent_disconnected() {
    persistent_connections().dec();
}

async fn metrics_handler() -> impl Responder {
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    let mut metrics = prometheus::gather();
    for m in &mut metrics {
        let name = format!("blind_eternities_{}", m.get_name());
        m.set_name(name);
    }
    if let Err(e) = encoder.encode(&metrics, &mut buffer) {
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
