use std::collections::HashMap;

use anyhow::Context;
use dirs::config_dir;

use crate::routing::Destination;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
pub struct Config {
    pub token: String,
    pub backend_url: String,
    #[serde(default)]
    pub network: Networking,
    #[serde(default)]
    pub default_user: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default, PartialEq, Eq)]
pub struct Networking {
    pub ssh: Option<u16>,
    pub aliases: HashMap<String, Destination>,
}

pub fn load_configuration() -> anyhow::Result<Config> {
    let mut settings = config::Config::default();

    let config_path = config_dir()
        .map(|mut d| {
            d.push("spark");
            d.push("config");
            d
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to find configuration file"))?;

    settings
        .merge(config::Environment::new().prefix("SPARK").separator("_"))?
        .merge(config::File::with_name(&config_path.display().to_string()).required(false))?;

    settings
        .try_into()
        .context("deserializing and creating settings")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn network_is_optional() {
        let conf = r#"{ "token": "token", "backend_url": "url" }"#;

        let conf = serde_json::from_str::<Config>(conf).expect("network should be fully optional");
        assert_eq!(conf.network.ssh, None);
        assert_eq!(conf.network.aliases, HashMap::default());
    }
}
