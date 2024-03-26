use std::{collections::HashMap, path::PathBuf};

use anyhow::Context;
use common::net::{
    auth_client::UrlParseError, defaults::default_persistent_conn_port, AuthenticatedClient,
};
use dirs::config_dir;
use url::Url;

use crate::util::destination::Destination;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
pub struct Config {
    pub token: uuid::Uuid,
    pub backend_domain: Url,
    #[serde(default)]
    pub network: Networking,
    #[serde(default)]
    pub persistent_conn: Option<PersistentConn>,
    #[serde(default)]
    pub default_user: Option<String>,
    #[serde(default = "crate::config::ipc_socket_path")]
    pub ipc_socket_path: PathBuf,
}

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
pub struct PersistentConn {
    #[serde(default = "::common::net::defaults::default_persistent_conn_port")]
    pub port: u16,
}

impl Default for PersistentConn {
    fn default() -> Self {
        Self {
            port: default_persistent_conn_port(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
pub struct Networking {
    #[serde(default)]
    pub ssh: Option<u16>,
    #[serde(default)]
    pub aliases: HashMap<String, Destination>,
}

impl TryFrom<&Config> for AuthenticatedClient {
    type Error = UrlParseError;
    fn try_from(c: &Config) -> Result<Self, Self::Error> {
        AuthenticatedClient::new(c.token, c.backend_domain.clone())
    }
}

const PREFIX: &str = "SPARK";

fn ipc_socket_path() -> PathBuf {
    let (path, e) = namespaced_tmp::blocking::in_tmp("spark", "socket");
    if let Some(e) = e {
        panic!("error creating ipc socket dir: {e:?}");
    } else {
        path
    }
}

pub fn load_configuration() -> anyhow::Result<Config> {
    let config_path = if let Ok(p) = std::env::var("SPARK__CONFIG_FILE") {
        PathBuf::from(p)
    } else {
        config_dir()
            .map(|mut d| {
                d.extend(["spark", "config"]);
                d
            })
            .ok_or_else(|| anyhow::anyhow!("Failed to find configuration file"))?
    };

    tracing::debug!(?config_path);

    config::Config::builder()
        .add_source(config::File::with_name(&config_path.display().to_string()).required(false))
        .add_source(config::Environment::with_prefix(PREFIX).separator("__"))
        .build()
        .and_then(config::Config::try_deserialize)
        .context("deserializing and creating settings")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn network_is_optional() {
        let conf = r#"{
            "token": "e751e207-59a8-4797-ab04-e8884b67e68e",
            "backend_domain": "http://url",
            "backend_port": 8000
        }"#;
        let conf = serde_json::from_str::<Config>(conf).expect("network should be fully optional");
        assert_eq!(conf.network.ssh, None);
        assert_eq!(conf.network.aliases, HashMap::default());
    }
}
