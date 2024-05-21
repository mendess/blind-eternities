use std::sync::OnceLock;

use prometheus::{register_int_counter, register_int_counter_vec, IntCounter, IntCounterVec};

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

pub fn music_backend_request(cmd: &spark_protocol::music::MusicCmdKind) {
    static METRICS: OnceLock<IntCounterVec> = OnceLock::new();
    METRICS
        .get_or_init(|| {
            register_int_counter_vec!(
                "music_backend_request",
                "number of times that cache missed",
                &["cmd"]
            )
            .unwrap()
        })
        .with_label_values(&[match cmd {
            spark_protocol::music::MusicCmdKind::Frwd => "Frwd",
            spark_protocol::music::MusicCmdKind::Back => "Back",
            spark_protocol::music::MusicCmdKind::CyclePause => "CyclePause",
            spark_protocol::music::MusicCmdKind::ChangeVolume { .. } => "ChangeVolume",
            spark_protocol::music::MusicCmdKind::Current => "Current",
            spark_protocol::music::MusicCmdKind::Queue { .. } => "Queue",
            spark_protocol::music::MusicCmdKind::Now { .. } => "Now",
        }])
        .inc();
}
