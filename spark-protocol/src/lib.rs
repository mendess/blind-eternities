pub mod client;
pub mod server;
pub mod music;

use std::{borrow::Cow, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io;

pub use common::net::RecvError;

/// Hits the local spark instance
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Local<'s> {
    Reload,
    Music(music::MusicCmd<'s>),
}

/// Hits the spark instance in a remote machine
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Remote<'s> {
    pub machine: Cow<'s, str>,
    pub command: Local<'s>,
}

/// Hits a route in the backend and returns the response
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend<'s> {
    Music(Cow<'s, str>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Command<'s> {
    Local(Local<'s>),
    Remote(Remote<'s>),
    Backend(Backend<'s>),
}

impl<'s> From<Local<'s>> for Command<'s> {
    fn from(l: Local<'s>) -> Self {
        Self::Local(l)
    }
}

impl<'s> From<Remote<'s>> for Command<'s> {
    fn from(l: Remote<'s>) -> Self {
        Self::Remote(l)
    }
}

impl<'s> From<Backend<'s>> for Command<'s> {
    fn from(l: Backend<'s>) -> Self {
        Self::Backend(l)
    }
}

pub type Response = Result<SuccessfulResponse, ErrorResponse>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum SuccessfulResponse {
    Unit,
    MusicResponse(music::Response),
}

impl From<music::Response> for SuccessfulResponse {
    fn from(music: music::Response) -> Self {
        Self::MusicResponse(music)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorResponse {
    DeserializingCommand(String),
    DeserializingResponse(String),
    ForwardedError(String),
    RequestFailed(String),
    NetworkError(String),
    IoError(String),
    HttpError { status: u16, message: String },
}

async fn socket_path() -> io::Result<PathBuf> {
    let (path, e) = namespaced_tmp::async_impl::in_tmp("spark", "socket").await;
    if let Some(e) = e {
        Err(e)
    } else {
        Ok(path)
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use super::*;
    use tempfile::{NamedTempFile, TempPath};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn any_cmd_returns_ok() {
        let p = spawn_server();
        tokio::time::sleep(Duration::from_secs(1)).await;

        let response = client::Client::from(UnixStream::connect(&p).await.unwrap())
            .send(Local::Reload)
            .await
            .unwrap()
            .expect("end of file");
        assert_eq!(Ok(SuccessfulResponse::Unit), response);
    }

    #[tokio::test]
    async fn multiple_commands_get_multiple_responses() {
        let p = spawn_server();
        tokio::time::sleep(Duration::from_secs(1)).await;

        let mut c = client::Client::from(UnixStream::connect(&p).await.unwrap());
        for i in 0..10 {
            let response = c
                .send(Local::Reload)
                .await
                .unwrap_or_else(|e| panic!("i: {i}: {:?}", e))
                .unwrap_or_else(|| panic!("i: {i}: end of file"));
            assert_eq!(Ok(SuccessfulResponse::Unit), response, "i: {i}");
        }
    }

    fn spawn_server() -> TempPath {
        let path = NamedTempFile::new().unwrap().into_temp_path();
        #[allow(clippy::unnecessary_to_owned)]
        tokio::spawn(
            server::ServerBuilder::new()
                .with_path(path.to_path_buf())
                .serve(|_| async { Ok(SuccessfulResponse::Unit) }),
        );
        path
    }
}
