use actix_web::{HttpResponse, Responder};

use crate::auth;

pub async fn health_check(_: auth::Admin) -> impl Responder {
    HttpResponse::Ok()
}
