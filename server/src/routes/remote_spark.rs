use actix_web::{web, HttpResponse, ResponseError};

pub fn routes() -> actix_web::Scope {
    web::scope("/remote-spark").route("", web::post().to(send_remote))
}

#[derive(Debug, Clone, thiserror::Error)]
enum RemoteSparkError {}

impl ResponseError for RemoteSparkError {}

async fn send_remote() -> Result<HttpResponse, RemoteSparkError> {
    todo!()
}
