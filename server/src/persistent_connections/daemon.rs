use std::{
    any::type_name, convert::Infallible, future::Future, io, net::SocketAddr, sync::Arc,
    time::Duration,
};

use common::net::{
    MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, RecvError, WriteJsonLinesExt,
    PERSISTENT_CONN_RECV_TIMEOUT,
};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use spark_protocol::{Command, Response};
use sqlx::PgPool;
use tokio::{
    io::{AsyncWriteExt, BufReader, BufWriter},
    net::{
        tcp::{ReadHalf, WriteHalf},
        TcpListener, TcpStream,
    },
    sync::mpsc,
    time::timeout,
};
use tracing::{instrument, Instrument};

use crate::{auth, metrics, persistent_connections::connections::Request};

use super::{connections::Connections, ConnectionError};

const TIMEOUT: Duration = Duration::from_secs(PERSISTENT_CONN_RECV_TIMEOUT.as_secs() / 2);

macro_rules! send {
    ($writer:ident <- $msg:expr; {
        $e:pat => $on_error:expr,
        elapsed => $on_elapsed:expr $(,)?
    }) => {
        send!($writer <- $msg; {
            (),
            $e => $on_error,
            elapsed => $on_elapsed,
        })
    };
    ($writer:ident <- $msg:expr; {
        $on_success:expr,
        $e:pat => $on_error:expr,
        elapsed => $on_elapsed:expr $(,)?
    }) => {{
        #[allow(unused_imports)]
        use ::common::net::WriteJsonLinesExt;
        ::tracing::debug!("sending {:?}", $msg);
        match ::tokio::time::timeout(TIMEOUT, $writer.send($msg)).await {
            Ok(Ok(())) => $on_success,
            Ok(Err($e)) => {
                $on_error
            }
            Err(_elapsed) => {
                $on_elapsed
            }
        }
    }}
}

#[instrument("receiving from client", skip(reader, writer, verifier))]
async fn receive<T, V, R, W, F, E, Fut>(
    reader: &mut R,
    writer: &mut W,
    name: &str,
    verifier: F,
) -> Option<V>
where
    T: DeserializeOwned,
    R: ReadJsonLinesExt,
    W: WriteJsonLinesExt,
    E: std::error::Error,
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = Result<V, E>>,
{
    match timeout(TIMEOUT, reader.recv()).await {
        Ok(Ok(Some(h))) => match verifier(h).await {
            Ok(v) => send!(writer <- MetaProtocolAck::Ok; {
                Some(v),
                e => {
                    tracing::error!(?e);
                    None
                },
                elapsed => {
                    tracing::error!("timed out sending ok ack");
                    None
                },
            }),
            Err(e) => send!(writer <- MetaProtocolAck::InvalidValue(format!("{:?}", e)); {
                None,
                e => {
                    tracing::error!(?e, "reporting invalid value error");
                    None
                },
                elapsed => {
                    tracing::error!("timed out sending invalid value error");
                    None
                }
            }),
        },
        Ok(Ok(None)) => {
            tracing::debug!("received EOF when receiving from client");
            None
        }
        Err(_elapsed) => {
            tracing::error!(
                "timed out receiving message of type {}",
                std::any::type_name::<T>()
            );
            None
        }
        Ok(Err(RecvError::Io(e))) => {
            tracing::error!(
                ?e,
                "receiving message of type {}",
                std::any::type_name::<T>()
            );
            None
        }
        Ok(Err(RecvError::Serde(e))) => {
            tracing::error!(?e);
            let e = MetaProtocolAck::DeserializationError {
                expected_type: type_name::<T>().into(),
                error: e.to_string(),
            };
            send!(writer <- e; {
                e => tracing::error!(?e, "reporting error"),
                elapsed => tracing::error!("timed out reporting error"),
            });
            None
        }
    }
}

async fn handle_a_connection(
    conn: &mut TcpStream,
    addr: SocketAddr,
    db: Arc<PgPool>,
    connections: Arc<Connections>,
) -> Option<()> {
    // accept a connection
    let (mut reader, mut writer, addr) = {
        let (r, w) = conn.split();
        (BufReader::new(r), BufWriter::new(w), addr)
    };
    tracing::info!(%addr, "accepted a connection");

    // start protocol
    let (gen, hostname, rx) = async {
        let hostname = receive(
            &mut reader,
            &mut writer,
            "receiving syn",
            |MetaProtocolSyn { hostname, token }| async move {
                auth::check_token::<auth::Admin>(&db, token)
                    .await
                    .map(|_| hostname)
            },
        )
        .await?;

        let (gen, rx) = connections.insert(hostname.clone()).await;
        tracing::info!(connected_hostname = %hostname, "connection established");
        Some((gen, hostname, rx))
    }
    .instrument(tracing::info_span!("protocol", %addr))
    .await?;

    let _span = tracing::debug_span!(
        "handling new persistent connection",
        connected_hostname = %hostname,
        ?gen
    );

    async fn handle(
        read: &mut BufReader<ReadHalf<'_>>,
        write: &mut BufWriter<WriteHalf<'_>>,
        mut rx: mpsc::Receiver<Request>,
    ) -> io::Result<()> {
        while let Some((cmd, ch)) = rx.recv().await {
            if cmd != Command::Heartbeat {
                tracing::info!(?cmd, "received cmd");
            }
            send!(write <- &cmd; {
                e => return Err(e),
                elapsed => continue,
            });
            let response = match timeout(TIMEOUT, read.recv()).await {
                Ok(Ok(Some(r))) => r,
                Ok(Ok(None)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(Err(e)) => {
                    if let RecvError::Serde(e) = &e {
                        let msg = MetaProtocolAck::DeserializationError {
                            error: e.to_string(),
                            expected_type: type_name::<Response>().into(),
                        };
                        send!(write <- msg; {
                            e => return Err(e),
                            elapsed => {}
                        });
                    }
                    return Err(e.into());
                }
                Err(_elapsed) => continue,
            };
            tracing::debug!(?response, "received response");
            if let Err(r) = ch.send(response) {
                tracing::error!(?cmd, response = ?r, "one shot channel closed");
            }
            tracing::debug!("forwarded response");
        }
        Ok(())
    }

    if let Err(e) = handle(&mut reader, &mut writer, rx).await {
        tracing::error!(?e, "persistent connection errored out");
    }
    tracing::debug!("removing connection");
    connections.remove(hostname, gen).await;
    Some(())
}

async fn heartbeat_checker(connections: Arc<Connections>) {
    loop {
        tokio::time::sleep(PERSISTENT_CONN_RECV_TIMEOUT / 6).await;
        let connections = &connections;
        futures::stream::iter(connections.connected_hosts().await)
            .map({
                |(h, gen)| async move {
                    match connections
                        .request(&h, spark_protocol::Command::Heartbeat)
                        .await
                    {
                        Err(ConnectionError::NotFound) | Ok(_) => None,
                        Err(ConnectionError::ConnectionDropped) => Some((h, gen)),
                    }
                }
            })
            .buffer_unordered(usize::MAX)
            .filter_map(|x| async { x })
            .for_each(|(h, gen)| async move {
                tracing::warn!(machine = %h, "machine disconnected");
                connections.remove(h, gen).await;
            })
            .await;
    }
}

pub(super) async fn start(
    listener: TcpListener,
    connections: Arc<Connections>,
    db: Arc<PgPool>,
) -> io::Result<Infallible> {
    tracing::info!(
        "starting persistent connection manager at port {:?}",
        listener.local_addr().map(|a| a.port())
    );
    tokio::spawn(heartbeat_checker(connections.clone()));
    loop {
        let (mut conn, addr) = listener.accept().await?;
        metrics::live_persistent_connection_sockets().inc();
        let connections = connections.clone();
        let db = db.clone();
        tokio::spawn(async move {
            handle_a_connection(&mut conn, addr, db, connections).await;
            match tokio::time::timeout(Duration::from_secs(60), conn.shutdown()).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::error!(?e, "failed to shutdown conn"),
                Err(_timeout) => {
                    tracing::error!("timeout while shutting down persistent connection socket")
                }
            }
            metrics::live_persistent_connection_sockets().dec();
        });
    }
}
