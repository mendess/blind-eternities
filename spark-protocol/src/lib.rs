pub mod client;
pub mod music;
pub mod server;

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io;

pub use common::net::RecvError;

/// Command to send to a spark instance.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
pub enum Command {
    /// Reload the spark instance
    Reload,
    /// Used by the backend to test if a connection is live.
    Heartbeat,
    /// Remotely control the music of a device.
    Music(music::MusicCmd),
    /// Returns the running version
    Version,
}

// /// Hits the spark instance in a remote machine
// #[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
// #[cfg_attr(feature = "clap", derive(clap::Parser))]
// pub struct Remote {
//     pub machine: Hostname,
//     #[cfg_attr(feature = "clap", command(subcommand))]
//     pub command: Command,
// }

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

pub trait ResponseExt {
    fn display(&self) -> ResponseDisplay<'_>;
}

impl ResponseExt for Response {
    fn display(&self) -> ResponseDisplay<'_> {
        ResponseDisplay(self)
    }
}

pub struct ResponseDisplay<'s>(&'s Response);

impl fmt::Display for ResponseDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Err(e) => write!(f, "Error: {e:?}"),
            Ok(response) => match response {
                SuccessfulResponse::Unit => f.write_str("success"),
                SuccessfulResponse::Version(version) => f.write_str(version),
                SuccessfulResponse::MusicResponse(music_resp) => {
                    use music::{Chapter, Response::*};
                    match music_resp {
                        Title { title } => write!(f, "Now playing: {title}"),
                        PlayState { paused } => {
                            write!(f, "{}", if *paused { "paused" } else { "playing" })
                        }
                        Volume { volume } => write!(f, "volume: {volume}%"),
                        Current {
                            paused,
                            title,
                            chapter,
                            volume,
                            progress,
                        } => {
                            match chapter {
                                Some(Chapter { title, index }) => writeln!(
                                    f,
                                    "Now Playing:\nVideo: {title} Song: {index} - {title}"
                                )?,
                                None => writeln!(f, "Now Playing: {title}")?,
                            };
                            writeln!(
                                f,
                                "{} at {volume}% volume",
                                if *paused { "paused" } else { "playing" }
                            )?;
                            write!(f, "Progress: {progress:.2} %")
                        }
                        QueueSummary {
                            from,
                            moved_to,
                            current,
                        } => {
                            writeln!(f, "Queued to position {from}.")?;
                            writeln!(f, "--> moved to {moved_to}.")?;
                            writeln!(f, "Currently playing {current}")
                        }
                    }
                }
            },
        }
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
            .send(&Command::Reload)
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
                .send(&Command::Reload)
                .await
                .unwrap_or_else(|e| panic!("i: {i}: {:?}", e))
                .unwrap_or_else(|| panic!("i: {i}: end of file"));
            assert_eq!(Ok(SuccessfulResponse::Unit), response, "i: {i}");
        }
    }

    fn spawn_server() -> TempPath {
        let path = NamedTempFile::new().unwrap().into_temp_path();
        let to_path_buf = path.to_path_buf();
        tokio::spawn(async move {
            server::ServerBuilder::new()
                .with_path(to_path_buf)
                .serve(|_| async { Ok(SuccessfulResponse::Unit) })
                .await
                .unwrap()
                .await
        });
        path
    }
}
