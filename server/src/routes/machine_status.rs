use std::net::IpAddr;

use common::{Hostname, MacAddr, MachineStatus};
use actix_web::{web, HttpResponse, Responder, ResponseError};
use anyhow::Context;
use chrono::Utc;
use sqlx::PgPool;

#[derive(thiserror::Error, Debug)]
pub enum MachineStatusError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for MachineStatusError {}

#[tracing::instrument(
    name = "Logging a machine status",
    skip(status, conn),
    fields(
        status_hostname = ?status.hostname,
        status_ip = %status.local_ip,
    )
)]
pub async fn post(
    status: web::Json<MachineStatus>,
    conn: web::Data<PgPool>,
) -> Result<HttpResponse, MachineStatusError> {
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
    .await
    .context("Failed to execute query")?;
    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(name = "list machine status", skip(conn))]
pub async fn get(conn: web::Data<PgPool>) -> Result<HttpResponse, MachineStatusError> {
    let status = sqlx::query!("SELECT * from machine_status")
        .fetch_all(conn.get_ref())
        .await
        .context("failed to execute query")?;

    todo!()
    // Ok(HttpResponse::Ok().body(status).finish())
}
