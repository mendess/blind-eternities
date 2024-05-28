use std::{
    collections::{hash_map::Entry, HashMap},
    fmt,
    sync::{Arc, OnceLock},
};

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

impl actix_web::ResponseError for SharedError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        self.0.status_code()
    }
}

type ResponseReceiver = Receiver<Option<Result<spark_protocol::music::Response, SharedError>>>;

#[derive(Default)]
struct RequestCoalescer {
    inflight: Mutex<HashMap<(Target, MusicCmdKind), ResponseReceiver>>,
}

static REQUEST_COALESCER: OnceLock<RequestCoalescer> = OnceLock::new();

pub async fn request_coalesced(
    client: &Backend,
    target: Target,
    cmd: MusicCmdKind,
) -> Result<spark_protocol::music::Response, SharedError> {
    let request_coalescer = REQUEST_COALESCER.get_or_init(Default::default);
    let mut channel = 'wait: {
        let mut inflight = request_coalescer.inflight.lock().await;
        let ((target, cmd), channel) = match inflight.entry((target, cmd)) {
            Entry::Occupied(receiver) => break 'wait receiver.get().clone(),
            Entry::Vacant(slot) => {
                let (tx, rx) = watch::channel(None);
                let key = slot.key().clone();
                slot.insert(rx);
                (key, tx)
            }
        };
        drop(inflight);
        let result = super::request_from_backend(client, &target, cmd.clone())
            .await
            .map_err(Arc::new)
            .map_err(Into::into);

        let _ = channel.send(Some(result.clone()));
        request_coalescer
            .inflight
            .lock()
            .await
            .remove(&(target, cmd));

        return result;
    };

    let result = channel
        .wait_for(Option::is_some)
        .await
        .expect("channel should never be closed");
    result.clone().unwrap().map_err(Into::into)
}
