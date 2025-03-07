use std::collections::hash_map::Entry;

use std::sync::Arc;
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};

use anyhow::Context;
use axum::{Json, Router, extract::State, response::IntoResponse, routing};
use chrono::Utc;
use common::domain::machine_status::{self, IpConnection, MachineStatusFull};
use futures::stream::{StreamExt, TryStreamExt};
use http::StatusCode;
use sqlx::PgPool;

use crate::auth;

pub fn routes() -> Router<super::RouterState> {
    Router::new().route("/status", routing::get(get).post(post))
}

#[derive(thiserror::Error, Debug)]
pub enum MachineStatusError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl IntoResponse for MachineStatusError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

#[tracing::instrument(
    name = "Logging a machine status",
    skip(status, conn),
    fields(
        status_hostname = ?status.hostname,
        status_ip = %status.external_ip,
    )
)]
pub async fn post(
    _: auth::Admin,
    conn: State<Arc<PgPool>>,
    Json(status): Json<machine_status::MachineStatus>,
) -> Result<impl IntoResponse, MachineStatusError> {
    let mut transaction = conn.begin().await.context("Failed to create transaction")?;

    sqlx::query!(
        r#"INSERT INTO machine_status (hostname, external_ip, last_heartbeat, ssh_port, default_user)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (hostname) DO UPDATE
        SET external_ip = $2, last_heartbeat = $3, ssh_port = $4, default_user = $5
        "#,
        status.hostname.as_ref(),
        status.external_ip.to_string(),
        Utc::now().naive_utc(),
        status.ssh.map(i32::from),
        status.default_user,
    )
    .execute(transaction.as_mut())
    .await
    .context("Failed to execute query")?;

    sqlx::query!(
        r#"DELETE FROM ip_connection WHERE hostname = $1"#,
        status.hostname.as_ref()
    )
    .execute(transaction.as_mut())
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
        .execute(transaction.as_mut())
        .await
        .context("Failed to insert new ips")?;
    }
    transaction
        .commit()
        .await
        .context("Failed to commit transaction")?;
    Ok(StatusCode::OK)
}

#[tracing::instrument(name = "list machine status", skip(conn))]
pub async fn get(
    _: auth::Admin,
    conn: State<Arc<PgPool>>,
) -> Result<impl IntoResponse, MachineStatusError> {
    let status = sqlx::query!(
        r#"SELECT
            ms.hostname as "hostname!",
            external_ip as "external_ip!",
            last_heartbeat as "last_heartbeat!",
            local_ip as "local_ip?",
            gateway_ip as "gateway_ip?",
            ssh_port,
            gateway_mac,
            default_user
         FROM machine_status ms
         LEFT JOIN ip_connection ip ON ms.hostname = ip.hostname"#
    )
    .fetch(&**conn)
    .map(|e| e.context("failed to execute query"))
    .try_fold(
        HashMap::<String, MachineStatusFull>::new(),
        |mut acc, record| async move {
            let mut entry = acc.entry(record.hostname.clone());
            let ips = match entry {
                Entry::Occupied(ref mut s) => &mut s.get_mut().fields.ip_connections,
                Entry::Vacant(v) => {
                    let hostname = record.hostname.try_into().context("parse hostname")?;
                    let ssh = record
                        .ssh_port
                        .map(u16::try_from)
                        .transpose()
                        .context("parse port")?;
                    let external_ip = record.external_ip.parse().context("parse external ip")?;

                    &mut v
                        .insert(MachineStatusFull {
                            fields: machine_status::MachineStatus {
                                hostname,
                                ssh,
                                external_ip,
                                ip_connections: vec![],
                                default_user: record.default_user,
                            },
                            last_heartbeat: record.last_heartbeat.and_utc(),
                        })
                        .fields
                        .ip_connections
                }
            };

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
    .await?;

    Ok((StatusCode::OK, Json(status)))
}
