use std::{
    future::{ready, Future, IntoFuture, Ready},
    io,
    net::TcpListener,
    sync::OnceLock,
};

use actix_web::{
    body::MessageBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::Method,
    web, App, HttpServer,
};
use prometheus::{register_int_counter_vec, Encoder, IntCounterVec};

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

pub async fn metrics_handler() -> String {
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

    match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = ?e, "custom metrics aren't valid utf8");
            String::default()
        }
    }
}

pub fn start_metrics_endpoint() -> io::Result<impl Future<Output = io::Result<()>>> {
    Ok(
        HttpServer::new(|| App::new().route("/metrics", web::get().to(metrics_handler)))
            .listen(TcpListener::bind("0.0.0.0:9000")?)?
            .run()
            .into_future(),
    )
}

pub struct RequestMetrics;

impl<S, B> Transform<S, ServiceRequest> for RequestMetrics
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Transform = RequestMetricsMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestMetricsMiddleware(service)))
    }
}

pub struct RequestMetricsMiddleware<S>(S);

impl<S, B> Service<ServiceRequest> for RequestMetricsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    type Future = S::Future;
    type Error = S::Error;
    type Response = S::Response;

    fn call(&self, req: ServiceRequest) -> Self::Future {
        new_request(
            req.match_pattern().as_deref().unwrap_or("UNMATCHED"),
            req.method(),
        );
        self.0.call(req)
    }

    fn poll_ready(
        &self,
        ctx: &mut core::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(ctx)
    }
}
