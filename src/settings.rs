use std::path::PathBuf;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Server {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Store {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub store: Store,
    pub server: Server,
}

pub fn load_config(config_file: &str) -> Result<Settings, ConfigError> {
    let defaults = r#"
store:
    path: ./cache
server:
    host: localhost
    port: 8080
    "#;
    let settings = Config::builder()
        .add_source(File::from_str(defaults, config::FileFormat::Yaml).required(true))
        .add_source(File::with_name(config_file).required(false))
        .add_source(Environment::with_prefix("GACHIX").separator("_"))
        .build()?;
    settings.try_deserialize()
}
