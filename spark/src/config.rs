use std::collections::HashMap;

use anyhow::Context;
use dirs::config_dir;

use crate::util::destination::Destination;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
pub struct Config {
    pub token: uuid::Uuid,
    pub backend_domain: String,
    pub backend_port: u16,
    #[serde(default = "::common::net::defaults::default_persistent_conn_port")]
    pub persistent_conn_port: u16,
    #[serde(default)]
    pub enable_persistent_conn: bool,
    #[serde(default)]
    pub network: Networking,
    #[serde(default)]
    pub default_user: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
pub struct Networking {
    #[serde(default)]
    pub ssh: Option<u16>,
    #[serde(default)]
    pub aliases: HashMap<String, Destination>,
}

const PREFIX: &str = "SPARK";

pub fn load_configuration() -> anyhow::Result<Config> {
    let config_path = config_dir()
        .map(|mut d| {
            d.extend(["spark", "config"]);
            d
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to find configuration file"))?;

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
            "backend_domain": "url",
            "backend_port": 8000
        }"#;
        let conf = serde_json::from_str::<Config>(conf).expect("network should be fully optional");
        assert_eq!(conf.network.ssh, None);
        assert_eq!(conf.network.aliases, HashMap::default());
    }
}
