use std::io;

use common::{
    domain::Hostname,
    net::{ReadJsonLinesExt, WriteJsonLinesExt},
};
use spark_protocol::{ErrorResponse, Local, Response};

use super::TestApp;

use tokio::{
    io::{BufReader, BufWriter},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};

impl TestApp {
    pub async fn connect(&self, hostname: &Hostname) -> Device {
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
        w.send_raw(&self.token.to_string()).await.expect("writing uuid");

        let mut read = BufReader::new(r);
        read.recv::<()>().await.expect("read confirmation");

        Device {
            read,
            write: BufWriter::new(w),
        }
    }
}

pub struct Device {
    read: BufReader<OwnedReadHalf>,
    write: BufWriter<OwnedWriteHalf>,
}

impl Device {
    pub async fn recv(&mut self) -> io::Result<Local<'static>> {
        Ok(self.read.recv().await?)
    }

    pub async fn send(&mut self, r: Result<Response, ErrorResponse>) -> io::Result<()> {
        self.write.send(r).await
    }
}

#[macro_export]
macro_rules! timeout {
    ($fut:expr) => {
        timeout!(5 => $fut)
    };
    ($t:expr => $fut:expr) => {
        match ::tokio::time::timeout(::std::time::Duration::from_secs($t), $fut).await {
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
    };
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
