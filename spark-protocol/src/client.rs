use std::path::PathBuf;

use common::net::{ReadJsonLinesExt, RecvError, WriteJsonLinesExt};
use tokio::{
    io::{self, BufReader, BufWriter},
    net::{self, UnixStream},
};

use crate::{Command, Response};

use super::socket_path;

#[derive(Debug)]
pub struct Client {
    pub(crate) reader: BufReader<net::unix::OwnedReadHalf>,
    pub(crate) writer: BufWriter<net::unix::OwnedWriteHalf>,
}

impl Client {
    #[inline(always)]
    pub async fn send(&mut self, cmd: &Command) -> Result<Option<Response>, RecvError> {
        self.writer.send(cmd).await?;
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

pub async fn send(cmd: &Command) -> Result<Option<Response>, RecvError> {
    let path = socket_path().await?;
    let socket = UnixStream::connect(path).await?;
    Client::from(socket).send(cmd).await
}
