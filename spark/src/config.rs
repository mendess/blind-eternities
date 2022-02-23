use std::collections::HashMap;

use dirs::config_dir;
use once_cell::sync::Lazy;

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

static CONFIG: Lazy<anyhow::Result<Config>> = Lazy::new(|| {
    let mut settings = config::Config::default();

    let config_path = config_dir()
        .map(|mut d| {
            d.push("spark");
            d.push("config");
            d
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to find configuration file"))?;

    settings.merge(config::File::with_name(&config_path.display().to_string()))?;

    Ok(settings.try_into()?)
});

pub fn load_configuration() -> Result<&'static Config, &'static anyhow::Error> {
    CONFIG.as_ref()
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
