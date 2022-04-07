pub mod auth_client;

use std::io;

pub use auth_client::AuthenticatedClient;
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

#[async_trait::async_trait]
pub trait WriteJsonLinesExt {
    async fn send<T: Serialize + Send>(&mut self, t: T) -> io::Result<()>;
}

#[async_trait::async_trait]
pub trait ReadJsonLinesExt {
    async fn recv<T: DeserializeOwned>(&mut self) -> io::Result<T>;
}

#[async_trait::async_trait]
impl<R> ReadJsonLinesExt for R
where
    R: AsyncBufReadExt + Unpin + Send,
{
    async fn recv<T: DeserializeOwned>(&mut self) -> io::Result<T> {
        loop {
            let buf = self.fill_buf().await?;
            if let Some(i) = buf.iter().position(|b| *b == b'\n') {
                let r = serde_json::from_slice(&buf[..i])?;
                self.consume(i + 1);
                break Ok(r);
            }
        }
    }
}

#[async_trait::async_trait]
impl<W> WriteJsonLinesExt for W
where
    W: AsyncWriteExt + Unpin + Send,
{
    async fn send<T: Serialize + Send>(&mut self, t: T) -> io::Result<()> {
        // TODO: allow buffer reuse or don't use a buffer at all
        let serialized = serde_json::to_vec(&t)?;
        self.write_all(&serialized).await?;
        self.write_all(b"\n").await?;
        self.flush().await?;
        Ok(())
    }
}
