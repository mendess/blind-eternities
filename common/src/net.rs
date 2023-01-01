pub mod auth_client;

use std::{
    fmt::{Debug, Display},
    io,
    ops::Deref,
    str::FromStr,
};

pub use auth_client::AuthenticatedClient;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::domain::Hostname;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaProtocolSyn {
    pub hostname: Hostname,
    pub token: uuid::Uuid,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaProtocolAck {
    Ok,
    DeserializationError {
        expected_type: String,
        error: String,
    },
    BadToken(String),
    InvalidValue(String),
}

#[derive(thiserror::Error, Debug)]
pub enum RecvError {
    #[error("IO({0})")]
    Io(#[from] io::Error),
    #[error("Serde({0})")]
    Serde(#[from] serde_json::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum RecvParseError<T>
where
    T: FromStr,
    T::Err: Debug + Display,
{
    #[error("IO({0})")]
    Io(#[from] io::Error),
    #[error("Utf8Error({0})")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("ParseError({0})")]
    ParseError(T::Err),
}

impl From<RecvError> for io::Error {
    fn from(e: RecvError) -> Self {
        match e {
            RecvError::Serde(e) => e.into(),
            RecvError::Io(e) => e,
        }
    }
}

#[async_trait::async_trait]
pub trait TalkJsonLinesExt {
    async fn talk<T: Serialize + Send, R: DeserializeOwned>(
        &mut self,
        t: T,
    ) -> Result<Option<R>, RecvError>;
}

#[async_trait::async_trait]
pub trait WriteJsonLinesExt {
    async fn send<T: Serialize + Send>(&mut self, t: T) -> io::Result<()>;
    async fn send_raw<S: AsRef<[u8]> + Send>(&mut self, s: S) -> io::Result<()>;
}

#[async_trait::async_trait]
pub trait ReadJsonLinesExt {
    async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, RecvError>;

    async fn recv_parse<T: FromStr>(&mut self) -> Result<Option<T>, RecvParseError<T>>
    where
        T::Err: Debug + Display;

    async fn recv_raw(&mut self) -> io::Result<Option<LineGuard<'_, Self>>>
    where
        Self: Sized + AsyncBufRead + Unpin;
}

#[derive(Debug)]
pub struct LineGuard<'s, T: AsyncBufRead + Unpin> {
    reader: &'s mut T,
    len: usize,
}

impl<'s, R: Unpin + AsyncRead> Deref for LineGuard<'s, BufReader<R>> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.reader.buffer()[..self.len]
    }
}

impl<'s, R: Unpin + AsyncRead> LineGuard<'s, BufReader<R>> {
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.reader.buffer()[..self.len]).expect("should have been a str")
    }

    pub fn as_str_checked(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.reader.buffer()[..self.len])
    }
}

impl<T: AsyncBufRead + Unpin> Drop for LineGuard<'_, T> {
    fn drop(&mut self) {
        self.reader.consume(self.len + 1)
    }
}

#[async_trait::async_trait]
impl<R: AsyncRead + Unpin + Send> ReadJsonLinesExt for BufReader<R> {
    async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, RecvError> {
        let line = match self.recv_raw().await? {
            Some(line) => line,
            None => return Ok(None),
        };
        tracing::debug!(line = ?line.deref(), "deserializing");
        Ok(serde_json::from_slice(&line)?)
    }

    async fn recv_parse<T: FromStr>(&mut self) -> Result<Option<T>, RecvParseError<T>>
    where
        T::Err: Debug + Display,
    {
        match self.recv_raw().await? {
            Some(line) => line
                .as_str_checked()?
                .parse()
                .map_err(RecvParseError::ParseError)
                .map(Some),
            None => return Ok(None),
        }
    }

    async fn recv_raw(&mut self) -> io::Result<Option<LineGuard<'_, Self>>> {
        loop {
            let buf = self.fill_buf().await?;
            match buf {
                [] => break Ok(None),
                _ => {
                    if let Some(len) = buf.iter().position(|b| *b == b'\n') {
                        break Ok(Some(LineGuard { reader: self, len }));
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl<W> WriteJsonLinesExt for W
where
    W: AsyncWrite + Unpin + Send,
{
    async fn send<T>(&mut self, t: T) -> io::Result<()>
    where
        T: Serialize + Send,
    {
        // TODO: allow buffer reuse or don't use a buffer at all
        let serialized = serde_json::to_vec(&t)?;
        self.send_raw(&serialized).await
    }

    async fn send_raw<T>(&mut self, bytes: T) -> io::Result<()>
    where
        T: AsRef<[u8]> + Send,
    {
        let bytes = bytes.as_ref();
        debug_assert_eq!(
            bytes.iter().position(|b| *b == b'\n'),
            None,
            "{:?} should not have '\n'",
            bytes
        );
        let mut buf = Vec::with_capacity(bytes.len() + 1);
        buf.extend_from_slice(bytes);
        buf.push(b'\n');
        self.write_all(&buf).await?;
        self.flush().await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<Reader, Writer> TalkJsonLinesExt for (&mut BufReader<Reader>, Writer)
where
    BufReader<Reader>: ReadJsonLinesExt + Send,
    Writer: WriteJsonLinesExt + Send,
{
    async fn talk<T: Serialize + Send, R: DeserializeOwned>(
        &mut self,
        t: T,
    ) -> Result<Option<R>, RecvError> {
        self.1.send(t).await?;
        self.0.recv().await
    }
}

pub mod defaults {
    pub const fn default_persistent_conn_port() -> u16 {
        2773
    }
}
