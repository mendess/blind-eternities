use std::{
    convert::Infallible,
    io,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use common::{
    domain::Hostname,
    net::{ReadJsonLinesExt, WriteJsonLinesExt},
};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::{
    io::{BufReader, BufWriter},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
    sync::{mpsc, oneshot},
    time::timeout,
};

pub type Request = (
    spark_protocol::Local<'static>,
    oneshot::Sender<Result<spark_protocol::Response, spark_protocol::ErrorResponse>>,
);

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

pub(crate) static CONNECTIONS: Lazy<DashMap<Hostname, (usize, mpsc::Sender<Request>)>> =
    Lazy::new(DashMap::new);

pub async fn start(listener: TcpListener) -> io::Result<Infallible> {
    static GENERATION: AtomicUsize = AtomicUsize::new(0);

    tracing::info!(
        "starting persistent connection manager at port {:?}",
        listener.local_addr().map(|a| a.port())
    );
    loop {
        // accept a connection
        let (mut reader, mut writer, addr) = {
            let (conn, addr) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!(?e, "error accepting a connection");
                    continue;
                }
            };
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

            let (tx, rx) = mpsc::channel::<Request>(100);
            let gen = GENERATION.fetch_add(1, Ordering::SeqCst);
            CONNECTIONS.insert(hostname.clone(), (gen, tx));

            if let Err(e) = writer.send(()).await {
                tracing::error!(?e, "failed to confirm handshake");
                continue;
            }
            tracing::info!(connected_machine = %hostname, "connection established");
            (gen, hostname, rx)
        };

        tokio::spawn(async move {
            let _span = tracing::debug_span!(
                "handling new persistent connection",
                connected_hostname = %hostname,
                %gen
            );
            if let Err(e) = handle(reader, writer, rx).await {
                tracing::error!(?e, "persistent connection errored out");
            }
            CONNECTIONS.remove_if(&hostname, |_, (g, _)| *g == gen);
        });
    }
}
