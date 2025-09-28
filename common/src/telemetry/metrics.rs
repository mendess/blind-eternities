use axum::{Router, response::IntoResponse, routing::get};
use http::{HeaderMap, HeaderValue, StatusCode, header};
use parking_lot::RwLock;
use prometheus_client::registry::Registry;
use std::{
    future::{Future, IntoFuture},
    io,
    sync::OnceLock,
};
use tokio::net::TcpListener;

#[doc(hidden)]
pub use parking_lot::MappedRwLockReadGuard;

pub static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();

#[macro_export]
macro_rules! make_metric {
    ($help:expr, $name:ident ($kind:path), {$($label:ident : $type:ty),*}$(,)?) => {
        make_metric!(
            $help,
            $name ($kind) ( $($label: $type),* ),
            { $($label: $type),* },
            { $($label),* }
        );
    };
    (
        $help:expr,
        $name:ident ($kind:path)($($label_param:ident : $type_param:ty),*),
        { $($label_struct:ident : $type_struct:ty),* },
        { $($conv:tt)* }
    ) => {
        pub fn $name($($label_param:$type_param),*)
            -> $crate::telemetry::metrics::MappedRwLockReadGuard<'static, $kind> {
            use ::prometheus_client::{
                metrics::{
                    family::Family
                },
                encoding::EncodeLabelSet
            };
            use ::std::sync::LazyLock;

            #[derive(Debug, Clone, Hash, Eq, PartialEq, EncodeLabelSet)]
            struct Labels {
                $($label_struct:$type_struct),*
            }
            static METRICS: LazyLock<Family<Labels, $kind>> = LazyLock::new(|| {
                let metric = Family::<Labels, $kind>::default();
                let Some(registry) = $crate::telemetry::metrics::REGISTRY.get() else {
                    return metric;
                };
                registry
                    .write()
                    .register(
                        ::std::stringify!($name),
                        $help,
                        metric.clone(),
                    );
                metric
            });
            METRICS.get_or_create(&Labels { $($conv)* })
        }
    };
}

pub async fn metrics_handler(axum: axum_prometheus::Handle) -> impl IntoResponse {
    let mut body = String::new();

    prometheus_client::encoding::text::encode_registry(&mut body, &REGISTRY.get().unwrap().read())
        .unwrap();

    body.push('\n');
    body.push_str(&axum.0.render());

    prometheus_client::encoding::text::encode_eof(&mut body).unwrap();

    (
        StatusCode::OK,
        HeaderMap::from_iter([(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )]),
        body,
    )
}

pub struct MetricsEndpoint<F> {
    pub worker: F,
    pub layer: axum_prometheus::GenericMetricLayer<
        'static,
        axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
        axum_prometheus::Handle,
    >,
}

pub fn start_metrics_endpoint(
    prefix: &'static str,
    metrics_listener: TcpListener,
) -> MetricsEndpoint<impl Future<Output = io::Result<()>>> {
    let (layer, handle) = axum_prometheus::PrometheusMetricLayerBuilder::new()
        .with_prefix(prefix)
        .with_endpoint_label_type(axum_prometheus::EndpointLabel::MatchedPathWithFallbackFn(
            |_| "UNMATCHED".into(),
        ))
        .with_default_metrics()
        .build_pair();

    REGISTRY.get_or_init(|| RwLock::new(Registry::with_prefix(prefix)));

    let worker = axum::serve(
        metrics_listener,
        Router::new().route(
            "/metrics",
            get(move || metrics_handler(axum_prometheus::Handle(handle.clone()))),
        ),
    )
    .into_future();

    MetricsEndpoint { worker, layer }
}
