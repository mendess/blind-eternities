use axum::{Router, routing::get};
use std::{
    future::{Future, IntoFuture},
    io,
};
use tokio::net::TcpListener;

pub use metrics::{
    Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};

pub struct MetricsEndpoint<F> {
    pub worker: F,
    pub layer: axum_prometheus::GenericMetricLayer<
        'static,
        axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
        axum_prometheus::Handle,
    >,
}

pub fn start_metrics_endpoint(
    metrics_listener: TcpListener,
) -> MetricsEndpoint<impl Future<Output = io::Result<()>>> {
    let (layer, handle) = axum_prometheus::PrometheusMetricLayerBuilder::new()
        .with_endpoint_label_type(axum_prometheus::EndpointLabel::MatchedPathWithFallbackFn(
            |_| "UNMATCHED".into(),
        ))
        .with_default_metrics()
        .build_pair();

    let collector = metrics_process::Collector::default();
    // Call `describe()` method to register help string.
    collector.describe();

    let worker = axum::serve(
        metrics_listener,
        Router::new().route(
            "/metrics",
            get(|| async move {
                collector.collect();
                handle.render()
            }),
        ),
    )
    .into_future();

    MetricsEndpoint { worker, layer }
}
