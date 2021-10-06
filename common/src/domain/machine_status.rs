use super::Hostname;
use super::MacAddr;
use std::net::IpAddr;

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
