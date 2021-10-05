use super::Hostname;
use super::MacAddr;
use std::net::IpAddr;
use chrono::{NaiveDate, NaiveTime, NaiveDateTime};

#[derive(
    serde::Deserialize,
    serde::Serialize,
    //TODO: delete?
    sqlx::FromRow,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub struct MachineStatus {
    pub hostname: Hostname,
    pub ip_connections: Vec<IpConnection>,
    pub external_ip: IpAddr,
    #[serde(default = "default_naive_time")]
    pub last_heartbeat: NaiveDateTime,
}

#[derive(
    serde::Deserialize,
    serde::Serialize,
    //TODO: delete?
    sqlx::FromRow,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub struct IpConnection {
    pub local_ip: IpAddr,
    pub gateway_ip: IpAddr,
    pub gateway_mac: Option<MacAddr>,
}

fn default_naive_time() -> NaiveDateTime {
    NaiveDateTime::new(
        NaiveDate::from_ymd(0, 1, 1),
        NaiveTime::from_hms(0, 0, 0),
    )
}

// impl<'r, R> sqlx::FromRow<'r, R> for MachineStatus
// where
//     R: Row,
//     String: sqlx::Decode<'r, R::Database>,
//     Hostname: sqlx::Decode<'r, R::Database>,
//     MacAddr: sqlx::Decode<'r, R::Database>,
// {
//     fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
//         let m = MachineStatus {
//             hostname: row.try_get::<Hostname>("hostname")?,
//             local_ip: row.try_get::<String>("local_ip")?.parse()?,
//             external_ip: row.try_get::<String>("external_ip").parse()?,
//             gateway_ip: row.try_get::<String>("gateway_ip").parse()?,
//             gateway_mac: row.try_get::<String>("gateway_mac").parse()?,
//         };
//         Ok(m)
//     }
// }
