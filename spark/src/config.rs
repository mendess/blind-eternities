use dirs::config_dir;
use once_cell::sync::Lazy;

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    pub token: String,
    pub backend_url: String,
    pub network: Option<Networking>,
}

#[derive(Debug, Copy, Clone, serde::Deserialize)]
pub struct Networking {
    pub ssh: u16,
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
