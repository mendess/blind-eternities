use common::make_metric;
use prometheus_client::metrics;

make_metric!(
    "number of times that the cache hit",
    music_cache_hit(metrics::counter::Counter),
    {}
);
make_metric!(
    "number of times that the cache missed",
    music_cache_miss(metrics::counter::Counter),
    {}
);
make_metric!(
    "number of times that the cache missed",
    music_backend_request(metrics::counter::Counter)(cmd: &spark_protocol::music::MusicCmdKind),
    { cmd: &'static str },
    { cmd: match cmd {
            spark_protocol::music::MusicCmdKind::Frwd => "Frwd",
            spark_protocol::music::MusicCmdKind::Back => "Back",
            spark_protocol::music::MusicCmdKind::CyclePause => "CyclePause",
            spark_protocol::music::MusicCmdKind::ChangeVolume { .. } => "ChangeVolume",
            spark_protocol::music::MusicCmdKind::Current => "Current",
            spark_protocol::music::MusicCmdKind::Queue { .. } => "Queue",
            spark_protocol::music::MusicCmdKind::Now { .. } => "Now",
        }
    }
);
