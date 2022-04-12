use std::{convert::Infallible, io, sync::Arc, time::Duration};

use common::{
    domain::Hostname,
    net::{ReadJsonLinesExt, WriteJsonLinesExt},
};
use sqlx::PgPool;
use tokio::{
    io::{BufReader, BufWriter},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
    sync::mpsc,
    time::timeout,
};

use crate::{
    auth::{self, AuthError},
    persistent_connections::connections::Request,
};

use super::connections::Connections;

async fn handle(
    mut read: BufReader<OwnedReadHalf>,
    mut write: BufWriter<OwnedWriteHalf>,
    mut rx: mpsc::Receiver<Request>,
) -> io::Result<()> {
    while let Some((cmd, ch)) = rx.recv().await {
        tracing::debug!(?cmd, "received cmd");
        write.send(&cmd).await?;
        let response = read.recv().await?;
        tracing::debug!(?response, "received response");
        if let Err(r) = ch.send(response) {
            tracing::error!(?cmd, response = ?r, "one shot channel closed");
        }
        tracing::debug!("forwarded response");
    }
    Ok(())
}

pub(crate) async fn start(
    listener: TcpListener,
    connections: Arc<Connections>,
    conn: Arc<PgPool>,
) -> io::Result<Infallible> {
    tracing::info!(
        "starting persistent connection manager at port {:?}",
        listener.local_addr().map(|a| a.port())
    );
    loop {
        // accept a connection
        let (mut reader, mut writer, addr) = {
            let (conn, addr) = listener.accept().await?;
            let (r, w) = conn.into_split();
            (BufReader::new(r), BufWriter::new(w), addr)
        };
        tracing::info!(%addr, "accepted a connection");

        // start protocol
        let (gen, hostname, rx) = {
            let _span = tracing::info_span!("protocol", %addr);
            let hostname = match timeout(Duration::from_secs(1), reader.recv::<Hostname>()).await {
                Ok(Err(e)) => {
                    tracing::error!(?e, %addr, "failed reading start protocol line");
                    continue;
                }
                Err(_elapsed) => {
                    tracing::info!(%addr, "start of protocol timed out");
                    continue;
                }
                Ok(Ok(h)) => h,
            };
            let token =
                match timeout(Duration::from_secs(1), reader.recv_parse::<uuid::Uuid>()).await {
                    Ok(Err(e)) => {
                        tracing::error!(?e, %addr, "failed reading auth token");
                        continue;
                    }
                    Err(_elapsed) => {
                        tracing::info!(%addr, "reading of auth token timed out");
                        continue;
                    }
                    Ok(Ok(h)) => h,
                };

            if let Err(e) = auth::check_token(&*conn, token).await {
                match e {
                    AuthError::InvalidToken => {
                        tracing::info!(connected_hostname = %hostname, %token, "invalid token");
                        continue;
                    }
                    e => {
                        tracing::error!(?e, connected_hostname = %hostname, %token, "error");
                        continue;
                    }
                }
            };

            if let Err(e) = writer.send(()).await {
                tracing::error!(?e, "failed to confirm handshake");
                continue;
            }
            tracing::info!(connected_hostname = %hostname, "connection established");
            let (gen, rx) = connections.insert(hostname.clone());
            (gen, hostname, rx)
        };

        tokio::spawn({
            let connections = connections.clone();
            async move {
                let _span = tracing::debug_span!(
                    "handling new persistent connection",
                    connected_hostname = %hostname,
                    %gen
                );
                if let Err(e) = handle(reader, writer, rx).await {
                    tracing::error!(?e, "persistent connection errored out");
                }
                connections.remove(&hostname, gen);
            }
        });
    }
}
