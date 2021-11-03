use super::Hostname;
use super::MacAddr;
use std::net::IpAddr;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MachineStatus {
    pub hostname: Hostname,
    pub ip_connections: Vec<IpConnection>,
    #[serde(default)]
    pub ssh: Option<u16>,
    pub external_ip: IpAddr,
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
