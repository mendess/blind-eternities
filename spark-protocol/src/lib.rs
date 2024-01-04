pub mod client;
pub mod music;
pub mod server;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::io;

pub use common::net::RecvError;

/// Hits the local spark instance
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub enum Local {
    /// Reload the spark instance
    Reload,
    /// Used by the backend to test if a connection is live.
    Heartbeat,
    /// Remotely control the music of a device.
    Music(music::MusicCmd),
}

/// Hits the spark instance in a remote machine
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct Remote {
    pub machine: String,
    #[cfg_attr(feature = "clap", command(subcommand))]
    pub command: Local,
}

/// Hits a route in the backend and returns the response
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub enum Backend {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
pub enum Command {
    #[cfg_attr(feature = "clap", command(flatten))]
    Local(Local),
    Remote(Remote),
    #[cfg_attr(feature = "clap", command(flatten))]
    Backend(Backend),
}

impl From<Local> for Command {
    fn from(l: Local) -> Self {
        Self::Local(l)
    }
}

impl From<Remote> for Command {
    fn from(l: Remote) -> Self {
        Self::Remote(l)
    }
}

impl From<Backend> for Command {
    fn from(l: Backend) -> Self {
        Self::Backend(l)
    }
}

pub type Response = Result<SuccessfulResponse, ErrorResponse>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum SuccessfulResponse {
    Unit,
    Version(String),
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
