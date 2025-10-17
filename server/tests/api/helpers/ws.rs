use super::TestApp;
use common::{domain::Hostname, ws};
use futures::{FutureExt as _, executor::block_on};
use rust_socketio::asynchronous::ClientBuilder;
use serde_json::json;
use spark_protocol::{Command, Response};
use tokio::sync::mpsc;

impl TestApp {
    pub async fn connect_device_ws(&self, hostname: &Hostname) -> Device {
        tracing::debug!("connecting to web socket as {hostname}");
        let (tx, rx) = mpsc::channel(1);
        let socket = ClientBuilder::new(format!("{}?h={hostname}", self.address))
            .auth(json!({ "token": self.auth_token }))
            .namespace(ws::NS)
            .on_with_ack(ws::COMMAND, move |payload, socket, ack| {
                let tx = tx.clone();
                async move {
                    tx.send((payload, socket, ack)).await.unwrap();
                }
                .boxed()
            })
            .on("error", |err, _| panic!("error occurred: {err:?}"))
            .connect()
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        tracing::debug!("simulated device connected");
        Device {
            read: rx,
            write: socket,
        }
    }
}

pub struct Device {
    read: mpsc::Receiver<(
        rust_socketio::Payload,
        rust_socketio::asynchronous::Client,
        rust_socketio::AckId,
    )>,
    write: rust_socketio::asynchronous::Client,
}

impl Drop for Device {
    fn drop(&mut self) {
        block_on(self.write.disconnect()).unwrap();
    }
}

impl Device {
    pub async fn recv(&mut self) -> Option<(Command, Reply)> {
        let (payload, socket, ack_id) = self.read.recv().await?;
        let rust_socketio::Payload::Text(mut v) = payload else {
            panic!("unexpected payload type");
        };
        Some((
            serde_json::from_value(v.remove(0)).unwrap(),
            Reply { socket, ack_id },
        ))
    }
}

pub struct Reply {
    socket: rust_socketio::asynchronous::Client,
    ack_id: rust_socketio::AckId,
}

impl Reply {
    pub async fn reply(self, r: Response) {
        self.socket
            .ack(self.ack_id, serde_json::to_string(&r).unwrap())
            .await
            .unwrap();
    }
}

#[macro_export]
macro_rules! timeout {
    ($fut:expr) => {
        timeout!(15 => $fut)
    };
    ($t:expr => $fut:expr) => {{
        let x = ::tokio::time::timeout(::std::time::Duration::from_secs($t), $fut).await;
        match x {
            Ok(x) => x,
            Err(_) => {
                ::std::panic!(
                    "\n{}\n",
                    ::std::concat!(
                        "[",
                        ::std::file!(),
                        ":",
                        ::std::line!(),
                        "] ",
                        ::std::stringify!($fut),
                        " timedout"
                    )
                )
            }
        }
    }};
}

#[macro_export]
macro_rules! assert_status {
    ($expected:expr, $got:expr) => {{
        let expected = $expected;
        let got = $got;
        assert_eq!(
            expected,
            got,
            "expected {} got {}",
            expected
                .canonical_reason()
                .unwrap_or_else(|| expected.as_str()),
            got.canonical_reason().unwrap_or_else(|| got.as_str()),
        )
    }};
}
