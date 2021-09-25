use std::net::IpAddr;

use crate::domain::Hostname;
use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use common::mac::MacAddr;
use sqlx::PgPool;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MachineStatus {
    hostname: Hostname,
    local_ip: IpAddr,
    external_ip: IpAddr,
    gateway_ip: IpAddr,
    gateway_mac: Option<MacAddr>,
}

#[tracing::instrument(
    name = "Logging a machine status",
    skip(status, conn),
    fields(
        status_hostname = ?status.hostname,
        status_ip = %status.local_ip,
    )
)]
pub async fn machine_status(
    status: web::Json<MachineStatus>,
    conn: web::Data<PgPool>,
) -> impl Responder {
    let status = status.into_inner();
    let result = sqlx::query!(r#"
    INSERT INTO machine_status (hostname, local_ip, external_ip, gateway_ip, gateway_mac, last_heartbeat)
    VALUES ($1, $2, $3, $4, $5, $6)"#,
        status.hostname.as_ref(),
        status.local_ip.to_string(),
        status.external_ip.to_string(),
        status.gateway_ip.to_string(),
        status.gateway_mac.map(|x| x.to_string()),
        Utc::now().naive_utc(),
    )
    .execute(conn.get_ref())
    .await;
    match result {
        Ok(_) => HttpResponse::Ok(),
        Err(e) => {
            tracing::error!("Failed to execute query: {:?}", e);
            HttpResponse::BadRequest()
        }
    }
}
