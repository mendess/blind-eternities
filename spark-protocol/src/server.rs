use crate::socket_path;
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

use super::{Command, ErrorResponse, Response};

struct Client {
    reader: BufReader<net::unix::OwnedReadHalf>,
    writer: BufWriter<net::unix::OwnedWriteHalf>,
}

impl Client {
    async fn recv(&mut self) -> io::Result<Option<Command>> {
        let mut s = String::new();
        let cmd = loop {
            s.clear();
            match self.reader.read_line(&mut s).await? {
                0 => return Ok(None),
                _ => match serde_json::from_str(&s) {
                    Ok(cmd) => break cmd,
                    Err(e) => {
                        self.send(Err(ErrorResponse::DeserializingCommand(e.to_string())))
                            .await?;
                    }
                },
            }
        };
        Ok(Some(cmd))
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

    pub async fn serve<F, Fut>(self, handler: F) -> io::Result<()>
    where
        F: Fn(Command) -> Fut + Clone + Send + 'static,
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
                    while let Some(cmd) = client.recv().await? {
                        let response = handler(cmd);
                        client.send(response.await).await?;
                    }
                    io::Result::Ok(())
                }
            });
        }
    }
}

pub async fn server<F, Fut>(handler: F) -> io::Result<()>
where
    F: Fn(Command) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<Response, ErrorResponse>> + Send + 'static,
{
    ServerBuilder::new().serve(handler).await
}
