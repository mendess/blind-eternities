use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::atomic::{AtomicU64, Ordering},
};

use common::domain::Hostname;
use spark_protocol::{Command, ErrorResponse};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::metrics;

pub(super) type Request = (Command, oneshot::Sender<Response>);

pub type Response = Result<spark_protocol::SuccessfulResponse, ErrorResponse>;

#[derive(PartialEq, Eq, Debug, Clone, Copy, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    pub fn next() -> Self {
        static GENERATION: AtomicU64 = AtomicU64::new(0);

        Self(GENERATION.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Debug)]
pub struct Connections {
    connected_hosts: Mutex<HashMap<Hostname, (Generation, mpsc::Sender<Request>)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("connection dropped. reason: {0:?}")]
    ConnectionDropped(Option<String>),
    #[error("not found")]
    NotFound,
}

impl Connections {
    pub(crate) fn new() -> Self {
        Self {
            connected_hosts: Default::default(),
        }
    }

    pub(crate) async fn request<C>(
        &self,
        machine: &Hostname,
        command: C,
    ) -> Result<spark_protocol::Response, ConnectionError>
    where
        C: Into<Command>,
    {
        let command = command.into();
        let channel = self
            .connected_hosts
            .lock()
            .await
            .get(machine)
            .map(|(_, ch)| ch.clone());
        match channel {
            Some(channel) => {
                let (tx, rx) = oneshot::channel();
                let log_infos = command != Command::Heartbeat;
                if log_infos {
                    tracing::info!("sending spark command");
                }
                channel
                    .send((command, tx))
                    .await
                    .map_err(|_| ConnectionError::ConnectionDropped(None))?;
                if log_infos {
                    tracing::info!("waiting for response");
                }
                let resp = rx
                    .await
                    .map_err(|e| ConnectionError::ConnectionDropped(Some(e.to_string())))?;
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

    #[tracing::instrument(skip(self))]
    pub(super) async fn insert(&self, machine: Hostname) -> (Generation, mpsc::Receiver<Request>) {
        let (tx, rx) = mpsc::channel::<Request>(100);
        let gen = Generation::next();
        self.connected_hosts.lock().await.insert(machine, (gen, tx));
        metrics::persistent_connected();
        (gen, rx)
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove(&self, machine: Hostname, gen: Generation) {
        if let Entry::Occupied(o) = self.connected_hosts.lock().await.entry(machine) {
            if o.get().0 == gen {
                o.remove_entry();
                metrics::persistent_disconnected();
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
