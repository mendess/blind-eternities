use std::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use common::domain::Hostname;
use dashmap::DashMap;
use spark_protocol::{ErrorResponse, Local};
use tokio::{
    sync::{mpsc, oneshot},
    time::timeout,
};

pub(super) type Request = (Local<'static>, oneshot::Sender<Response>);

pub type Response = Result<spark_protocol::Response, ErrorResponse>;

#[derive(Debug, Clone)]
pub(crate) struct Connections {
    map: DashMap<Hostname, (usize, mpsc::Sender<Request>)>,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectionError {
    #[error("timedout")]
    Timedout,
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("not found")]
    NotFound,
}

impl Connections {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            map: Default::default(),
            timeout,
        }
    }

    pub(crate) async fn request(
        &self,
        machine: &Hostname,
        command: Local<'static>,
    ) -> Result<Response, ConnectionError> {
        match self.map.get(machine) {
            Some(conn) => {
                let (tx, rx) = oneshot::channel();
                tracing::info!("sending spark command");
                conn.1
                    .send((command, tx))
                    .await
                    .map_err(|_| ConnectionError::ConnectionDropped)?;
                tracing::info!("waiting for response");
                let resp = timeout(self.timeout, rx)
                    .await
                    .map_err(|_| ConnectionError::Timedout)?
                    .map_err(|_| ConnectionError::ConnectionDropped)?;
                tracing::info!(?resp, "received response");
                Ok(resp)
            }
            None => {
                tracing::info!("hostname not connected");
                Err(ConnectionError::NotFound)
            }
        }
    }

    pub(super) fn insert(&self, machine: Hostname) -> (usize, mpsc::Receiver<Request>) {
        static GENERATION: AtomicUsize = AtomicUsize::new(0);
        let (tx, rx) = mpsc::channel::<Request>(100);
        let gen = GENERATION.fetch_add(1, Ordering::SeqCst);
        self.map.insert(machine, (gen, tx));
        (gen, rx)
    }

    pub(super) fn remove(&self, machine: &Hostname, gen: usize) {
        self.map.remove_if(&machine, |_, (g, _)| *g == gen);
    }
}
