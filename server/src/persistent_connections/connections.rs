use std::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

use common::domain::Hostname;
use dashmap::DashMap;
use spark_protocol::{ProtocolError, Local};
use tokio::sync::{mpsc, oneshot};

pub(super) type Request = (Local<'static>, oneshot::Sender<Response>);

pub type Response = Result<spark_protocol::ProtocolMsg, ProtocolError>;

#[derive(Debug, Clone)]
pub(crate) struct Connections {
    map: DashMap<Hostname, (usize, mpsc::Sender<Request>)>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectionError {
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("not found")]
    NotFound,
}

impl Connections {
    pub(crate) fn new() -> Self {
        Self {
            map: Default::default(),
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
                let resp = rx.await.map_err(|_| ConnectionError::ConnectionDropped)?;
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
        self.map.remove_if(machine, |_, (g, _)| *g == gen);
    }
}
