use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

use common::domain::Hostname;
use spark_protocol::{ErrorResponse, Local};
use tokio::sync::{mpsc, oneshot, Mutex};

pub(super) type Request = (Local, oneshot::Sender<Response>);

pub type Response = Result<spark_protocol::SuccessfulResponse, ErrorResponse>;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Generation(usize);

impl Generation {
    fn new() -> Self {
        static GENERATION: AtomicUsize = AtomicUsize::new(0);

        Self(GENERATION.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Debug)]
pub struct Connections {
    connected_hosts: Mutex<HashMap<Hostname, (Generation, mpsc::Sender<Request>)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("not found")]
    NotFound,
}

impl Connections {
    pub(crate) fn new() -> Self {
        Self {
            connected_hosts: Default::default(),
        }
    }

    pub(crate) async fn request(
        &self,
        machine: &Hostname,
        command: Local,
    ) -> Result<Response, ConnectionError> {
        match self.connected_hosts.lock().await.get(machine) {
            Some(conn) => {
                let (tx, rx) = oneshot::channel();
                let log_infos = command != Local::Heartbeat;
                if log_infos {
                    tracing::info!("sending spark command");
                }
                conn.1
                    .send((command, tx))
                    .await
                    .map_err(|_| ConnectionError::ConnectionDropped)?;
                if log_infos {
                    tracing::info!("waiting for response");
                }
                let resp = rx.await.map_err(|_| ConnectionError::ConnectionDropped)?;
                if log_infos {
                    tracing::info!(?resp, "received response");
                }
                Ok(resp)
            }
            None => {
                tracing::info!("hostname not connected");
                Err(ConnectionError::NotFound)
            }
        }
    }

    pub(super) async fn insert(&self, machine: Hostname) -> (Generation, mpsc::Receiver<Request>) {
        let (tx, rx) = mpsc::channel::<Request>(100);
        let gen = Generation::new();
        self.connected_hosts.lock().await.insert(machine, (gen, tx));
        (gen, rx)
    }

    pub async fn remove(&self, machine: Hostname, gen: Generation) {
        if let Entry::Occupied(o) = self.connected_hosts.lock().await.entry(machine) {
            if o.get().0 == gen {
                o.remove_entry();
            }
        }
    }

    pub async fn connected_hosts(&self) -> Vec<(Hostname, Generation)> {
        self.connected_hosts
            .lock()
            .await
            .iter()
            .map(|(k, (gen, _))| (k.clone(), *gen))
            .collect()
    }
}
