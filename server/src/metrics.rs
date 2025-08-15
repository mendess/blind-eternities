use common::make_metric;
use prometheus_client::metrics;

make_metric!(
    "Number of open sockets for persistent connections",
    live_persistent_connection_sockets(metrics::gauge::Gauge),
    {}
);

make_metric!(
    "Number of persistent_connections online",
    persistent_connections(metrics::gauge::Gauge),
    {}
);
