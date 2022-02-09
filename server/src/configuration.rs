#[derive(Debug, serde::Deserialize)]
pub struct Settings {
    pub port: u16,
    pub db: DbSettings,
    #[serde(default)]
    pub allow_any_localhost_token: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct DbSettings {
    pub username: String,
    pub password: String,
    pub port: u16,
    pub host: String,
    pub name: String,
    #[serde(default)]
    pub migrate: bool,
}

impl DbSettings {
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.name
        )
    }

    pub fn connection_string_without_db(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

pub const PREFIX: &str = "BLIND_ETER";

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let mut settings = config::Config::default();

    settings.merge(config::File::with_name("configuration").required(false))?;
    settings.merge(config::Environment::with_prefix(PREFIX).separator("_"))?;

    settings.try_into()
}
