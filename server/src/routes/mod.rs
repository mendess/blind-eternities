pub mod admin;
pub mod machine_status;
pub mod music;
pub mod persistent_connections;
pub mod playlist;

use std::{path::PathBuf, sync::Arc};

use axum::{Router, extract::FromRef};
use sqlx::PgPool;

use crate::persistent_connections::ws::SocketIo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, FromRef)]
pub struct RouterState {
    playlist_config: Arc<PlaylistConfig>,
    db: Arc<PgPool>,
    socket_io: SocketIo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistConfig {
    pub song_dir: PathBuf,
}

pub fn router(db: Arc<PgPool>, socket_io: SocketIo, playlist_config: PlaylistConfig) -> Router {
    Router::new()
        .nest("/admin", admin::routes())
        .nest("/machine", machine_status::routes())
        .nest("/persistent-connections", persistent_connections::routes())
        .nest("/music", music::routes())
        .nest("/playlist", playlist::routes())
        .with_state(RouterState {
            db,
            socket_io,
            playlist_config: Arc::new(playlist_config),
        })
}
