use std::sync::OnceLock;

use prometheus::{register_int_counter, IntCounter};

pub fn cache_hit() {
    static METRICS: OnceLock<IntCounter> = OnceLock::new();
    METRICS
        .get_or_init(|| {
            register_int_counter!("cache_hit", "number of times that cache hits").unwrap()
        })
        .inc();
}

pub fn cache_miss() {
    static METRICS: OnceLock<IntCounter> = OnceLock::new();
    METRICS
        .get_or_init(|| {
            register_int_counter!("cache_miss", "number of times that cache missed").unwrap()
        })
        .inc();
}
