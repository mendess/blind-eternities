use chrono::NaiveDateTime;

use super::Hostname;
use super::MacAddr;
use std::net::IpAddr;
use std::ops::Deref;
use std::ops::DerefMut;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MachineStatus {
    pub hostname: Hostname,
    pub ip_connections: Vec<IpConnection>,
    #[serde(default)]
    pub ssh: Option<u16>,
    pub external_ip: IpAddr,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MachineStatusFull {
    #[serde(flatten)]
    pub fields: MachineStatus,
    pub last_heartbeat: NaiveDateTime,
}

impl Deref for MachineStatusFull {
    type Target = MachineStatus;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl DerefMut for MachineStatusFull {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fields
    }
}

impl MachineStatus {
    pub fn is_port_forwarded(&self) -> bool {
        self.ssh.is_some()
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IpConnection {
    pub local_ip: IpAddr,
    pub gateway_ip: IpAddr,
    #[serde(default)]
    pub gateway_mac: Option<MacAddr>,
}
