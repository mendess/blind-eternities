use actix_web::{web, HttpResponse, Responder};

use crate::persistent_connections::Connections;

pub fn routes() -> actix_web::Scope {
    web::scope("/persistent-connections").route("", web::get().to(index))
}

async fn index(connections: web::Data<Connections>) -> impl Responder {
    let connected = connections.connected_hosts().await;
    HttpResponse::Ok().json(connected)
}
