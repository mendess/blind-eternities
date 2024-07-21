#[cfg(feature = "metrics")]
pub mod metrics;

use opentelemetry::KeyValue;
use opentelemetry_sdk::{trace, Resource};
use tracing::{dispatcher::set_global_default, Subscriber};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::{
    fmt::{self, MakeWriter},
    layer::SubscriberExt,
    registry::LookupSpan,
    EnvFilter, Registry,
};

pub fn with_tracing<S>(subscriber: S, service_name: String) -> impl Subscriber + Sync + Send
where
    S: Subscriber + Sync + Send + for<'span> LookupSpan<'span>,
{
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_trace_config(
            trace::config()
                .with_resource(Resource::new([KeyValue::new("service.name", service_name)])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("couldn't create OTLP tracer");

    let layer = tracing_opentelemetry::layer().with_tracer(tracer);

    subscriber.with(layer)
}

pub fn get_subscriber<W: for<'a> MakeWriter<'a> + Send + Sync + 'static>(
    name: String,
    env_filter: String,
    sink: W,
) -> impl Subscriber + Sync + Send + for<'span> LookupSpan<'span> {
    LogTracer::init().expect("Failed to set logger");

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));
    let formatting_layer = BunyanFormattingLayer::new(name, sink);

    Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(formatting_layer)
}

pub fn get_subscriber_no_bunny(env_filter: String) -> impl Subscriber + Sync + Send {
    LogTracer::init().expect("Failed to set logger");

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

    let fmt = fmt::layer().event_format(fmt::format()).pretty();

    Registry::default().with(env_filter).with(fmt)
}

pub fn init_subscriber(subscriber: impl Subscriber + Sync + Send) {
    set_global_default(subscriber.into()).expect("Failed to set global default");
}
