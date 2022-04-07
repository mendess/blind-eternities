use std::time::Duration;

use actix_web::{http::StatusCode, web, HttpResponse, ResponseError};
use common::domain::Hostname;
use spark_protocol::Local;
use tokio::{sync::oneshot, time::timeout};

use crate::persistent_connections::CONNECTIONS;

pub fn routes() -> actix_web::Scope {
    web::scope("/remote-spark/{hostname}").route("", web::post().to(send_remote))
}

#[derive(Debug, Clone, thiserror::Error)]
enum RemoteSparkError {
    #[error("connection dropped")]
    ConnectionDropped,
    #[error("timed out")]
    Timedout,
}

impl ResponseError for RemoteSparkError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ConnectionDropped => StatusCode::NOT_FOUND,
            Self::Timedout => StatusCode::REQUEST_TIMEOUT,
        }
    }
}

#[tracing::instrument("sending remote command")]
async fn send_remote(
    machine: web::Path<Hostname>,
    cmd: web::Json<Local<'static>>,
) -> Result<HttpResponse, RemoteSparkError> {
    match CONNECTIONS.get(&machine) {
        Some(conn) => {
            let (tx, rx) = oneshot::channel();
            tracing::info!("sending spark command");
            conn.1
                .send((cmd.0, tx))
                .await
                .map_err(|_| RemoteSparkError::ConnectionDropped)?;
            tracing::info!("waiting for response");
            let resp = timeout(Duration::from_secs(5), rx)
                .await
                .map_err(|_| RemoteSparkError::Timedout)?
                .map_err(|_| RemoteSparkError::ConnectionDropped)?;
            tracing::info!(?resp, "received response");
            Ok(HttpResponse::Ok().json(resp))
        }
        None => {
            tracing::info!("hostname not connected");
            Ok(HttpResponse::NotFound().finish())
        }
    }
}
