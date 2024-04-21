use actix_web::{web, HttpResponse, Responder};
use common::domain::Hostname;

use crate::{
    auth,
    persistent_connections::{ConnectionError, Connections},
};

pub fn routes() -> actix_web::Scope {
    web::scope("/persistent-connections")
        .route("", web::get().to(list_persistent_connections))
        .route("/send/{hostname}", web::post().to(send))
}

async fn list_persistent_connections(
    _: auth::Admin,
    connections: web::Data<Connections>,
) -> impl Responder {
    let connected = connections.connected_hosts().await;
    HttpResponse::Ok().json(connected.into_iter().map(|(h, _)| h).collect::<Vec<_>>())
}

async fn send(
    _: auth::Admin,
    connections: web::Data<Connections>,
    web::Json(command): web::Json<spark_protocol::Command>,
    hostname: web::Path<Hostname>,
) -> impl Responder {
    let r = connections.request(&hostname, command).await;
    tracing::debug!(response = ?r, "responding");
    match r {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(ConnectionError::NotFound) => HttpResponse::NotFound().into(),
        Err(ConnectionError::ConnectionDropped) => HttpResponse::InternalServerError().into(),
    }
}
