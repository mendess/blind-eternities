use std::io;

use super::TestApp;
use common::{
    domain::Hostname,
    net::{
        MetaProtocolAck, MetaProtocolSyn, ReadJsonLinesExt, TalkJsonLinesExt, WriteJsonLinesExt,
    },
};
use spark_protocol::{Command, Response};
use tokio::{
    io::{BufReader, BufWriter},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

impl TestApp {
    pub async fn connect_device(&self, hostname: &Hostname) -> Device {
        tracing::debug!(
            "connecting to port {} as {}",
            self.persistent_conn_port,
            hostname
        );
        let socket = TcpStream::connect(("localhost", self.persistent_conn_port))
            .await
            .expect("can't connect");

        let (r, mut w) = socket.into_split();

        let mut r = BufReader::new(r);

        let mut talker = (&mut r, &mut w);

        let syn = MetaProtocolSyn {
            hostname: hostname.clone(),
            token: self.auth_token,
        };
        tracing::debug!(?syn, "sending syn");
        assert_eq!(
            MetaProtocolAck::Ok,
            talker
                .talk(syn)
                .await
                .expect("writing hostname")
                .expect("eof"),
        );

        tracing::debug!("simulated device connected");
        Device {
            read: r,
            write: BufWriter::new(w),
        }
    }
}

pub struct Device {
    read: BufReader<OwnedReadHalf>,
    write: BufWriter<OwnedWriteHalf>,
}

impl Device {
    pub async fn recv(&mut self) -> io::Result<Option<Command>> {
        Ok(self.read.recv().await?)
    }

    pub async fn send(&mut self, r: Response) -> io::Result<()> {
        self.write.send(r).await
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
