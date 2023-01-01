use std::{
    any::type_name, convert::Infallible, future::Future, io, net::SocketAddr, sync::Arc,
    time::Duration,
};

use common::net::{
    MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, RecvError, WriteJsonLinesExt,
};
use serde::de::DeserializeOwned;
use spark_protocol::{ErrorResponse, Response};
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
use tracing::instrument;

use crate::{
    auth::{self, is_localhost},
    persistent_connections::connections::Request,
};

use super::connections::Connections;

const TIMEOUT: Duration = Duration::from_secs(10);

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
    allow_any_localhost_token: bool,
) -> Option<()> {
    // accept a connection
    let (mut reader, mut writer, addr) = {
        let (r, w) = conn.split();
        (BufReader::new(r), BufWriter::new(w), addr)
    };
    tracing::info!(%addr, "accepted a connection");

    // start protocol
    let (gen, hostname, rx) = {
        let _span = tracing::info_span!("protocol", %addr);
        let hostname = receive(
            &mut reader,
            &mut writer,
            "receiving syn",
            |MetaProtocolSyn { hostname, token }| async move {
                if allow_any_localhost_token && is_localhost(addr) {
                    Ok(hostname)
                } else {
                    auth::check_token(&db, token).await.map(|_| hostname)
                }
            },
        )
        .await?;

        tracing::info!(connected_hostname = %hostname, "connection established");
        let (gen, rx) = connections.insert(hostname.clone()).await;
        (gen, hostname, rx)
    };

    let _span = tracing::debug_span!(
        "handling new persistent connection",
        connected_hostname = %hostname,
        %gen
    );

    async fn handle(
        read: &mut BufReader<ReadHalf<'_>>,
        write: &mut BufWriter<WriteHalf<'_>>,
        mut rx: mpsc::Receiver<Request>,
    ) -> io::Result<()> {
        while let Some((cmd, ch)) = rx.recv().await {
            tracing::info!(?cmd, "received cmd");
            send!(write <- &cmd; {
                e => return Err(e),
                elapsed => continue,
            });
            let response = match timeout(TIMEOUT, read.recv()).await {
                Ok(Ok(Some(r))) => r,
                Ok(Ok(None)) => Err(ErrorResponse::NetworkError(
                    "daemon closed the channel socket before responding".into(),
                )),
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
    connections.remove(hostname, gen).await;
    Some(())
}

pub(super) async fn start(
    listener: TcpListener,
    connections: Arc<Connections>,
    db: Arc<PgPool>,
    allow_any_localhost_token: bool,
) -> io::Result<Infallible> {
    tracing::info!(
        "starting persistent connection manager at port {:?}",
        listener.local_addr().map(|a| a.port())
    );
    loop {
        let (mut conn, addr) = listener.accept().await?;
        let connections = connections.clone();
        let db = db.clone();
        tokio::spawn(async move {
            handle_a_connection(&mut conn, addr, db, connections, allow_any_localhost_token).await;
            if let Err(e) = conn.shutdown().await {
                tracing::error!(?e, "failed to shutdown conn");
            }
        });
    }
}
