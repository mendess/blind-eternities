pub mod client;
pub mod server;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
#[cfg(feature = "structopt")]
use structopt::StructOpt;
use tokio::io;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "structopt", derive(StructOpt))]
pub enum Command {
    Reload,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Response {
    Success,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorResponse {
    DeserializingCommand(String),
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
            .send(Command::Reload)
            .await
            .unwrap();
        assert_eq!(Ok(Response::Success), response);
    }

    #[tokio::test]
    async fn multiple_commands_get_multiple_responses() {
        let p = spawn_server();
        tokio::time::sleep(Duration::from_secs(1)).await;

        let mut c = client::Client::from(UnixStream::connect(&p).await.unwrap());
        for i in 0..10 {
            let response = c
                .send(Command::Reload)
                .await
                .unwrap_or_else(|e| panic!("i: {i}: {:?}", e));
            assert_eq!(Ok(Response::Success), response, "i: {i}");
        }
    }

    fn spawn_server() -> TempPath {
        let path = NamedTempFile::new().unwrap().into_temp_path();
        #[allow(clippy::unnecessary_to_owned)]
        tokio::spawn(
            server::ServerBuilder::new()
                .with_path(path.to_path_buf())
                .serve(|_| async { Ok(Response::Success) }),
        );
        path
    }
}
