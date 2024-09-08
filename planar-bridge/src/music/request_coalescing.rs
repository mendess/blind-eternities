use std::{
    collections::{hash_map::Entry, HashMap},
    fmt,
    sync::{atomic::AtomicUsize, Arc, OnceLock},
};

use axum::response::IntoResponse;
use spark_protocol::music::MusicCmdKind;
use tokio::sync::{
    watch::{self, Receiver},
    Mutex,
};

use crate::Backend;

use super::Target;

#[derive(Debug, Clone)]
pub struct SharedError(Arc<super::Error>);

impl fmt::Display for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Arc<super::Error>> for SharedError {
    fn from(value: Arc<super::Error>) -> Self {
        Self(value)
    }
}

impl IntoResponse for SharedError {
    fn into_response(self) -> axum::response::Response {
        (self.0.status_code(), self.to_string()).into_response()
    }
}

type ResponseReceiver = Receiver<Option<Result<spark_protocol::music::Response, SharedError>>>;

#[derive(Default)]
struct RequestCoalescer {
    inflight: Mutex<HashMap<(Target, MusicCmdKind), (ResponseReceiver, usize)>>,
}

static REQUEST_COALESCER: OnceLock<RequestCoalescer> = OnceLock::new();

#[tracing::instrument(skip(client))]
pub async fn request_coalesced(
    client: &Backend,
    target: Target,
    cmd: MusicCmdKind,
) -> Result<spark_protocol::music::Response, SharedError> {
    static CH_ID: AtomicUsize = AtomicUsize::new(0);
    let request_coalescer = REQUEST_COALESCER.get_or_init(Default::default);
    let (mut channel, id) = 'wait: {
        let mut inflight = request_coalescer.inflight.lock().await;
        let ((target, cmd), channel, id) = match inflight.entry((target.clone(), cmd.clone())) {
            Entry::Occupied(receiver) => break 'wait receiver.get().clone(),
            Entry::Vacant(slot) => {
                let (tx, rx) = watch::channel(None);
                let key = slot.key().clone();
                let id = CH_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                slot.insert((rx, id));
                (key, tx, id)
            }
        };
        drop(inflight);
        let client = client.clone();
        // we spawn here to avoid having this be canceled. When it's canceled we might not remove
        // from the hashmap or we might not send to the channel. Causing the waiting code to crash
        // on the expect.
        let handle = tokio::spawn(async move {
            tracing::info!(?id, "requesting from backend");
            let result = super::request_from_backend(&client, &target, cmd.clone())
                .await
                .map_err(Arc::new)
                .map_err(Into::into);

            tracing::info!(?id, "sending result");
            let r = channel.send(Some(result.clone()));
            tracing::info!(?id, "success? {}", r.is_ok());
            request_coalescer
                .inflight
                .lock()
                .await
                .remove(&(target, cmd));
            tracing::info!(?id, "dropping channel");
            result
        });
        return handle.await.unwrap();
    };

    tracing::info!(?id, "waiting");

    let result = channel
        .wait_for(Option::is_some)
        .await
        .expect("channel should never be closed");
    result.clone().unwrap().map_err(Into::into)
}
