use std::sync::OnceLock;

use prometheus::{IntGauge, register_int_gauge};

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
