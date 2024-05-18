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
            Err(e) => {
                f.write_str("Error: ")?;
                let msg = match e {
                    ErrorResponse::DeserializingCommand(msg) => {
                        writeln!(f, "remote spark failed to deserialize your command")?;
                        msg
                    }
                    ErrorResponse::ForwardedError(msg) => {
                        writeln!(f, "remote spark failed to execute your command")?;
                        msg
                    }
                    ErrorResponse::RequestFailed(msg) => {
                        writeln!(f, "remote spark refused to execute your command")?;
                        msg
                    }
                    ErrorResponse::IoError(msg) => {
                        writeln!(
                            f,
                            "remote spark failed to execute your command due to an io error:"
                        )?;
                        msg
                    }
                    ErrorResponse::RelayError(msg) => {
                        writeln!(
                            f,
                            "the blind-eternities encountered an error while communicating with the remote spark"
                        )?;
                        msg
                    }
                };
                write!(f, " -> {msg}")
            }
            Ok(response) => match response {
                SuccessfulResponse::Unit => f.write_str("success"),
                SuccessfulResponse::Version(version) => f.write_str(version),
                SuccessfulResponse::MusicResponse(music_resp) => {
                    use music::Response::*;
                    match music_resp {
                        Title { title } => write!(f, "Now playing: {title}"),
                        PlayState { paused } => {
                            write!(f, "{}", if *paused { "paused" } else { "playing" })
                        }
                        Volume { volume } => write!(f, "volume: {volume}%"),
                        Current {
                            current:
                                mlib::queue::Current {
                                    title,
                                    chapter,
                                    playing,
                                    volume,
                                    progress,
                                    ..
                                },
                        } => {
                            match chapter {
                                Some((index, title)) => writeln!(
                                    f,
                                    "Now Playing:\nVideo: {title} Song: {index} - {title}"
                                )?,
                                None => writeln!(f, "Now Playing: {title}")?,
                            };
                            writeln!(
                                f,
                                "{} at {volume}% volume",
                                if *playing { "playing" } else { "paused" }
                            )?;
                            write!(f, "Progress: {:.2} %", progress.unwrap_or_default())
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
                        Now {
                            before,
                            current,
                            after,
                        } => {
                            for b in before {
                                writeln!(f, "   {b}")?;
                            }
                            writeln!(f, "-> {current}")?;
                            for b in after {
                                writeln!(f, "   {b}")?;
                            }
                            Ok(())
                        }
                    }
                }
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorResponse {
    /// The command could not be understood by the remote spark
    DeserializingCommand(String),
    /// The remote spark encountered a generic error and could not continue.
    ForwardedError(String),
    /// The remote spark refused to process the request
    RequestFailed(String),
    /// The remote spark encountered an io error and could not continue.
    IoError(String),
    /// The relay (blind-eternities) encountered an error when receiving the response from
    /// the remote spark.
    RelayError(String),
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
