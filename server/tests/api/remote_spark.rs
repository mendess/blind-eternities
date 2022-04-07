use std::io;

use crate::helpers::{fake_hostname, TestApp};
use common::{
    domain::Hostname,
    net::{ReadJsonLinesExt, WriteJsonLinesExt},
};
use fake::Fake;
use reqwest::StatusCode;
use spark_protocol::{ErrorResponse, Local, Response};
use tokio::{
    io::{BufReader, BufWriter},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};

impl TestApp {
    async fn send(&self, hostname: Hostname, cmd: Local<'_>) -> reqwest::Response {
        tracing::debug!("sending command {cmd:?} to {hostname}");
        self.post_authed(&format!("remote-spark/{hostname}"))
            .json(&cmd)
            .send()
            .await
            .expect("success")
    }

    async fn connect(&self, hostname: &Hostname) -> Device {
        tracing::debug!(
            "connecting to port {} as {}",
            self.persistent_conn_port,
            hostname
        );
        let socket = TcpStream::connect(("localhost", self.persistent_conn_port))
            .await
            .expect("can't connect");

        let (r, mut w) = socket.into_split();

        w.send(&hostname).await.expect("writing hostname");

        let mut read = BufReader::new(r);
        read.recv::<()>().await.expect("read confirmation");

        Device {
            read,
            write: BufWriter::new(w),
        }
    }
}

struct Device {
    read: BufReader<OwnedReadHalf>,
    write: BufWriter<OwnedWriteHalf>,
}

impl Device {
    async fn recv(&mut self) -> io::Result<Local<'static>> {
        self.read.recv().await
    }

    async fn send(&mut self, r: Result<Response, ErrorResponse>) -> io::Result<()> {
        self.write.send(r).await
    }
}

macro_rules! timeout {
    ($fut:expr) => {
        timeout!(5 => $fut)
    };
    ($t:expr => $fut:expr) => {
        match ::tokio::time::timeout(::std::time::Duration::from_secs($t), $fut).await {
            Ok(x) => x,
            Err(_) => {
                ::std::panic!(
                    "{}",
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
    };
}

#[actix_rt::test]
async fn sending_a_valid_cmd_to_an_existing_conn_forwards_the_request() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname()
        .fake::<String>()
        .parse()
        .expect("invalid hostname");

    let mut device = timeout!(app.connect(&hostname));

    let join = tokio::spawn(async move {
        timeout!(20 => async {
            app.send(hostname, Local::Reload)
                .await
                .json::<Result<Response, ErrorResponse>>()
                .await
        })
    });

    let req = timeout!(device.recv()).expect("failed to receive");
    assert_eq!(req, Local::Reload);
    timeout!(device.send(Ok(Response::Unit))).expect("to send");

    let resp = timeout!(join).expect("failed to join").expect("deser");
    assert_eq!(resp, Ok(Response::Unit));
}

#[actix_rt::test]
async fn sending_a_command_to_a_non_existent_machine_404s() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname()
        .fake::<String>()
        .parse()
        .expect("invalid hostname");

    let response = timeout!(20 => app.send(hostname, Local::Reload));

    assert_eq!(StatusCode::NOT_FOUND, response.status(),)
}

#[actix_rt::test]
async fn sending_a_command_to_an_existing_but_unresponsive_machine_times_out() {
    let app = TestApp::spawn().await;

    let hostname = fake_hostname()
        .fake::<String>()
        .parse()
        .expect("invalid hostname");

    let _device = timeout!(app.connect(&hostname));

    let response = timeout!(20 => app.send(hostname, Local::Reload));

    assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status(),)
}
