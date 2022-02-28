use std::path::PathBuf;

use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{self, UnixStream},
};

use super::{socket_path, Command, ErrorResponse, Response};

pub struct Client {
    reader: BufReader<net::unix::OwnedReadHalf>,
    writer: BufWriter<net::unix::OwnedWriteHalf>,
}

impl Client {
    pub async fn send(&mut self, cmd: Command) -> io::Result<Result<Response, ErrorResponse>> {
        let payload = serde_json::to_vec(&cmd).expect("serialization to never fail");
        self.writer.write_all(&payload).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        loop {
            let buf = self.reader.fill_buf().await?;
            if let Some(i) = buf.iter().position(|b| *b == b'\n') {
                let r = serde_json::from_slice(&buf[..i])?;
                self.reader.consume(i + 1);
                break Ok(r);
            }
        }
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

pub async fn send(cmd: Command) -> io::Result<Result<Response, ErrorResponse>> {
    let path = socket_path().await?;
    let socket = UnixStream::connect(path).await?;
    Client::from(socket).send(cmd).await
}
