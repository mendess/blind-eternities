use std::mem::take;
use std::net::IpAddr;
use std::{collections::HashMap, convert::TryInto};

use actix_web::{web, HttpResponse, Responder, ResponseError};
use anyhow::Context;
use chrono::{NaiveDateTime, Utc};
use common::domain::{machine_status::IpConnection, Hostname, MacAddr, MachineStatus};
use futures::stream::{StreamExt, TryStreamExt};
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
        status_ip = %status.external_ip,
    )
)]
pub async fn post(
    status: web::Json<MachineStatus>,
    conn: web::Data<PgPool>,
) -> Result<HttpResponse, MachineStatusError> {
    let status = status.into_inner();
    let mut transaction = conn
        .get_ref()
        .begin()
        .await
        .context("Failed to create transaction")?;

    let result = sqlx::query!(
        r#"INSERT INTO machine_status (hostname, external_ip, last_heartbeat) VALUES ($1, $2, $3)"#,
        status.hostname.as_ref(),
        status.external_ip.to_string(),
        Utc::now().naive_utc(),
    )
    .execute(&mut transaction)
    .await
    .context("Failed to execute query")?;

    sqlx::query!(
        r#"DELETE FROM ip_connection WHERE hostname = $1"#,
        status.hostname.as_ref()
    )
    .execute(&mut transaction)
    .await
    .context("Failed to delete old ips")?;

    for c in status.ip_connections {
        sqlx::query!(
            r#"INSERT INTO ip_connection (hostname, local_ip, gateway_ip, gateway_mac)
            VALUES ($1, $2, $3, $4)"#,
            status.hostname.as_ref(),
            c.local_ip.to_string(),
            c.gateway_ip.to_string(),
            c.gateway_mac.map(|x| x.to_string()),
        )
        .execute(&mut transaction)
        .await
        .context("Failed to insert new ips")?;
    }
    transaction
        .commit()
        .await
        .context("Failed to commit transaction")?;
    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(name = "list machine status", skip(conn))]
pub async fn get(conn: web::Data<PgPool>) -> Result<HttpResponse, MachineStatusError> {
    let status = sqlx::query!(
        r#"SELECT
            ms.hostname as "hostname!",
            external_ip as "external_ip!",
            last_heartbeat as "last_heartbeat!",
            local_ip as "local_ip?",
            gateway_ip as "gateway_ip?",
            gateway_mac
         FROM machine_status ms
         LEFT JOIN ip_connection ip ON ms.hostname = ip.hostname"#
    )
    .fetch(conn.get_ref())
    .try_fold(
        HashMap::<String, MachineStatus>::new(),
        |mut acc, mut record| async move {
            let ips = &mut acc
                .entry(record.hostname.clone())
                .or_insert_with(|| MachineStatus {
                    hostname: take(&mut record.hostname).try_into().unwrap(),
                    external_ip: record.external_ip.parse().unwrap(),
                    last_heartbeat: record.last_heartbeat,
                    ip_connections: vec![],
                })
                .ip_connections;

            if let (Some(local_ip), Some(gateway_ip), gateway_mac) =
                (record.local_ip, record.gateway_ip, record.gateway_mac)
            {
                ips.push(IpConnection {
                    local_ip: local_ip.parse().unwrap(),
                    gateway_ip: gateway_ip.parse().unwrap(),
                    gateway_mac: gateway_mac.map(|x| x.parse()).transpose().unwrap(),
                });
            }
            Ok(acc)
        },
    )
    .await
    .context("failed to execute query")?;

    Ok(HttpResponse::Ok().json(status))
}
