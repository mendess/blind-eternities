use crate::{socket_path, Command};
use common::net::{ReadJsonLinesExt, RecvError, WriteJsonLinesExt};
use std::{
    fmt::Debug,
    fs::Permissions,
    future::Future,
    io,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::{
    fs,
    io::{BufReader, BufWriter},
    net::{self, UnixListener, UnixStream},
};

use super::ErrorResponse;

struct Client {
    reader: BufReader<net::unix::OwnedReadHalf>,
    writer: BufWriter<net::unix::OwnedWriteHalf>,
}

impl Client {
    #[inline(always)]
    async fn recv(&mut self) -> Result<Option<Command>, RecvError> {
        self.reader.recv().await
    }

    #[inline(always)]
    async fn send(&mut self, r: crate::Response) -> io::Result<()> {
        self.writer.send(&r).await
    }
}

impl From<UnixStream> for Client {
    fn from(s: UnixStream) -> Self {
        let (r, w) = s.into_split();
        Self {
            reader: BufReader::new(r),
            writer: BufWriter::new(w),
        }
    }
}

#[derive(Default, Debug)]
pub struct ServerBuilder {
    path: Option<PathBuf>,
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self { path: None }
    }

    pub fn with_path(self, path: PathBuf) -> ServerBuilder {
        ServerBuilder { path: Some(path) }
    }

    // TODO: move to spark and finish implementing
    pub async fn serve<F, Fut>(self, handler: F) -> io::Result<impl Future<Output = ()>>
    where
        F: Fn(Command) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = crate::Response> + Send,
    {
        async fn create_socket<P: AsRef<Path> + Debug>(p: P) -> io::Result<UnixListener> {
            if let Err(e) = fs::remove_file(&p).await {
                if e.kind() != io::ErrorKind::NotFound {
                    tracing::error!(?e, path = ?p, "failed to remove old socket");
                    return Err(e);
                }
            }
            tracing::info!(path = ?p, "binding ipc socket");
            let socket = UnixListener::bind(&p)?;
            fs::set_permissions(p, Permissions::from_mode(0o777)).await?;
            Ok(socket)
        }
        let socket = match &self.path {
            Some(p) => create_socket(p).await?,
            None => create_socket(socket_path().await?).await?,
        };
        Ok(async move {
            let mut id = 0;
            loop {
                let (client, _) = match socket.accept().await {
                    Ok(client) => client,
                    Err(e) => {
                        tracing::error!(?e, "failed to accept a connection.");
                        break;
                    }
                };
                let local_id = id;
                id += 1;
                tokio::spawn({
                    let handler = handler.clone();
                    async move {
                        let mut client = Client::from(client);
                        loop {
                            let rcv = client.recv().await;
                            tracing::info!(%local_id, req = ?rcv, "received local request");
                            match rcv {
                                Ok(Some(c)) => {
                                    let response = handler(c);
                                    client.send(response.await).await?;
                                }
                                Ok(None) => break,
                                Err(RecvError::Io(e)) => return Err(e),
                                Err(RecvError::Serde(s)) => {
                                    client
                                        .send(Err(ErrorResponse::DeserializingCommand(
                                            s.to_string(),
                                        )))
                                        .await?;
                                }
                            }
                        }
                        io::Result::Ok(())
                    }
                });
            }
        })
    }
}

pub async fn server<F, Fut>(handler: F) -> io::Result<impl Future<Output = ()>>
where
    F: Fn(Command) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = crate::Response> + Send + 'static,
{
    ServerBuilder::new().serve(handler).await
}
