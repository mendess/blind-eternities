use config::{Config, Environment, File};
use std::path::PathBuf;

#[derive(Debug, serde::Deserialize)]
pub struct Settings {
    pub port: u16,
    pub db: DbSettings,
    #[serde(default = "::common::net::defaults::default_persistent_conn_port")]
    pub persistent_conn_port: u16,
    #[serde(default = "enabled")]
    pub enable_metrics: bool,
    pub data_dir: PathBuf,
}

fn enabled() -> bool {
    true
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

pub fn get_configuration(path: Option<&str>) -> Result<Settings, config::ConfigError> {
    Config::builder()
        .add_source(File::with_name(path.unwrap_or("configuration")).required(false))
        .add_source(Environment::with_prefix(PREFIX).separator("__"))
        .build()
        .and_then(Config::try_deserialize)
}
