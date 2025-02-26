#[cfg(feature = "metrics")]
pub mod metrics;

use std::io;

use tracing::{Subscriber, dispatcher::set_global_default};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, MakeWriter},
    layer::SubscriberExt,
};

pub fn get_subscriber<W: for<'a> MakeWriter<'a> + Send + Sync + 'static>(
    name: String,
    env_filter: String,
    sink: W,
) -> impl Subscriber + Sync + Send {
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

    let fmt = fmt::layer()
        .with_writer(io::stderr)
        .event_format(fmt::format())
        .pretty();

    Registry::default().with(env_filter).with(fmt)
}

pub fn init_subscriber(subscriber: impl Subscriber + Sync + Send) {
    set_global_default(subscriber.into()).expect("Failed to set global default");
}
