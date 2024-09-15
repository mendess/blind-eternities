use std::sync::Arc;

use common::{domain::Hostname, ws};
use socketioxide::{
    extract::{Extension, SocketRef},
    handler::ConnectHandler,
    socket::DisconnectReason,
};

pub type SocketIo = socketioxide::SocketIo<socketioxide::adapter::LocalAdapter>;

pub type SHostname = Arc<Hostname>;

fn hostname_middleware(s: SocketRef) -> Result<(), &'static str> {
    tracing::info!("hostname middleware called");
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
        s.extensions.insert(hostname);
        Ok(())
    } else {
        Err("hostname missing")
    }
}

fn on_connect(socket: SocketRef, hostname: Extension<SHostname>) {
    tracing::info!(hostname = %*hostname, sid = %socket.id, "socket connected");

    socket.on_disconnect(
        |s: SocketRef, reason: DisconnectReason, hostname: Extension<SHostname>| {
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

pub fn socket_io_routes() -> (socketioxide::layer::SocketIoLayer, SocketIo) {
    let (layer, io) = socketioxide::SocketIo::new_layer();
    io.ns(ws::NS, on_connect.with(hostname_middleware));
    (layer, io)
}
