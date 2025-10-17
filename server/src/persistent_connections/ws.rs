use std::sync::Arc;

use common::{domain::Hostname, ws};
use serde::Deserialize;
use socketioxide::{
    extract::{Data, Extension, SocketRef, State},
    handler::ConnectHandler,
    socket::DisconnectReason,
};
use sqlx::PgPool;

use crate::{metrics, persistent_connections::Generation};

pub type SocketIo = socketioxide::SocketIo<socketioxide::adapter::LocalAdapter>;

pub type SHostname = Arc<Hostname>;

#[tracing::instrument(skip_all)]
async fn hostname_middleware(s: SocketRef) -> Result<(), &'static str> {
    #[derive(serde::Deserialize)]
    struct Q {
        #[serde(alias = "h")]
        hostname: Arc<Hostname>,
    }
    if let Some(Q { hostname }) =
        s.req_parts().uri.query().and_then(|q| {
            serde_querystring::from_str(q, serde_querystring::ParseMode::UrlEncoded).ok()
        })
    {
        tracing::info!("hostname connected {hostname}");
        metrics::persistent_connections().inc();
        s.extensions.insert(hostname);
        s.extensions.insert(Generation::next());
        Ok(())
    } else {
        Err("hostname missing")
    }
}

#[derive(Deserialize)]
struct Auth {
    token: uuid::Uuid,
}

#[tracing::instrument(skip_all, fields(auth = ?auth.token))]
async fn auth_middleware(
    auth: Data<Auth>,
    State(db): State<Arc<PgPool>>,
) -> Result<(), crate::auth::AuthError> {
    let r = crate::auth::check_token::<crate::auth::Admin>(&db, auth.token).await;
    tracing::info!("authenticated? {}", r.is_ok());
    Ok(())
}

#[tracing::instrument(skip_all)]
fn on_connect(socket: SocketRef, hostname: Extension<SHostname>) {
    tracing::info!(hostname = %*hostname, sid = %socket.id, "socket connected");

    socket.on_disconnect(
        |s: SocketRef, reason: DisconnectReason, hostname: Extension<SHostname>| {
            metrics::persistent_connections().dec();
            tracing::info!(
                hostname = %*hostname,
                sid = %s.id,
                ns = s.ns(),
                ?reason,
                "socket disconnected"
            );
        },
    );
}

pub fn socket_io_routes(db: Arc<PgPool>) -> (socketioxide::layer::SocketIoLayer, SocketIo) {
    let (layer, io) = socketioxide::SocketIo::builder()
        .with_state(db)
        .build_layer();
    io.ns(
        ws::NS,
        on_connect.with(hostname_middleware).with(auth_middleware),
    );
    (layer, io)
}
