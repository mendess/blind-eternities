use actix_web::{http::StatusCode, web, HttpResponse, ResponseError};
use common::domain::Hostname;
use spark_protocol::Local;

use crate::persistent_connections::{ConnectionError, Connections};

pub fn routes() -> actix_web::Scope {
    web::scope("/remote-spark/{hostname}").route("", web::post().to(send_remote))
}

impl ResponseError for ConnectionError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ConnectionDropped => StatusCode::REQUEST_TIMEOUT,
            Self::NotFound => StatusCode::NOT_FOUND,
        }
    }
}

#[tracing::instrument("sending remote command")]
async fn send_remote(
    machine: web::Path<Hostname>,
    cmd: web::Json<Local<'static>>,
    connections: web::Data<Connections>,
) -> Result<HttpResponse, ConnectionError> {
    let v = connections.request(&machine, cmd.0).await?;
    Ok(HttpResponse::Ok().json(v))
}
