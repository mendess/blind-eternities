use std::path::PathBuf;

use common::net::{ReadJsonLinesExt, RecvError, WriteJsonLinesExt};
use tokio::{
    io::{self, BufReader, BufWriter},
    net::{self, UnixStream},
};

use crate::Response;

use super::{socket_path, Command};

#[derive(Debug)]
pub struct Client {
    pub(crate) reader: BufReader<net::unix::OwnedReadHalf>,
    pub(crate) writer: BufWriter<net::unix::OwnedWriteHalf>,
}

impl Client {
    #[inline(always)]
    pub async fn send<'s, C>(&mut self, cmd: C) -> Result<Option<Response>, RecvError>
    where
        C: Into<Command<'s>>,
    {
        self.writer.send(&cmd.into()).await?;
        self.reader.recv().await
    }
}

#[derive(Default, Debug)]
pub struct ClientBuilder {
    path: Option<PathBuf>,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self { path: None }
    }

    pub fn with_path(&mut self, p: PathBuf) -> &mut Self {
        self.path = Some(p);
        self
    }

    pub async fn build(&mut self) -> io::Result<Client> {
        let socket = match &self.path {
            Some(p) => UnixStream::connect(p).await?,
            None => UnixStream::connect(socket_path().await?).await?,
        };
        Ok(Client::from(socket))
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

pub async fn send(cmd: Command<'_>) -> Result<Option<Response>, RecvError> {
    let path = socket_path().await?;
    let socket = UnixStream::connect(path).await?;
    Client::from(socket).send(cmd).await
}
