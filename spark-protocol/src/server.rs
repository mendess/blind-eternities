use crate::{socket_path, Command};
use std::{
    fs::Permissions,
    future::Future,
    io,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{self, UnixListener, UnixStream},
};

use super::{ErrorResponse, Response};

struct Client {
    reader: BufReader<net::unix::OwnedReadHalf>,
    writer: BufWriter<net::unix::OwnedWriteHalf>,
}

#[derive(thiserror::Error, Debug)]
enum RecvError {
    #[error("IO({0})")]
    Io(#[from] io::Error),
    #[error("Serde({0})")]
    Serde(#[from] serde_json::Error),
}

impl Client {
    async fn recv(&mut self) -> Result<Option<Command<'static>>, RecvError> {
        loop {
            let buf = self.reader.fill_buf().await?;
            if let Some(i) = buf.iter().position(|b| *b == b'\n') {
                let r = serde_json::from_slice(&buf[..i])?;
                self.reader.consume(i + 1);
                break Ok(r);
            }
        }
    }

    async fn send(&mut self, r: Result<Response, ErrorResponse>) -> io::Result<()> {
        let payload = serde_json::to_vec(&r).expect("serialization should never fail");
        self.writer.write_all(&payload).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
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
    pub async fn serve<F, Fut>(self, handler: F) -> io::Result<()>
    where
        F: Fn(Command<'static>) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = Result<Response, ErrorResponse>> + Send + 'static,
    {
        async fn create_socket<P: AsRef<Path>>(p: P) -> io::Result<UnixListener> {
            if let Err(e) = fs::remove_file(&p).await {
                if e.kind() != io::ErrorKind::NotFound {
                    return Err(e);
                }
            }
            println!("server listening on: {:?}", p.as_ref());
            let socket = UnixListener::bind(&p)?;
            fs::set_permissions(p, Permissions::from_mode(0o777)).await?;
            Ok(socket)
        }
        let socket = match &self.path {
            Some(p) => create_socket(p).await?,
            None => create_socket(socket_path().await?).await?,
        };
        loop {
            let (client, _) = socket.accept().await?;
            tokio::spawn({
                let handler = handler.clone();
                async move {
                    let mut client = Client::from(client);
                    loop {
                        match client.recv().await {
                            Ok(Some(c)) => {
                                let response = handler(c);
                                client.send(response.await).await?;
                            }
                            Ok(None) => break,
                            Err(RecvError::Io(e)) => return Err(e),
                            Err(RecvError::Serde(s)) => {
                                client
                                    .send(Err(ErrorResponse::DeserializingCommand(s.to_string())))
                                    .await?;
                            }
                        }
                    }
                    io::Result::Ok(())
                }
            });
        }
    }
}

pub async fn server<F, Fut>(handler: F) -> io::Result<()>
where
    F: Fn(Command<'static>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<Response, ErrorResponse>> + Send + 'static,
{
    ServerBuilder::new().serve(handler).await
}
