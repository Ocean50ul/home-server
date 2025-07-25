use serde::Deserialize;
use std::{fs, path::PathBuf};
use toml;
use std::sync::OnceLock;
use anyhow::{anyhow, Error};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub media: MediaConfig
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub path: PathBuf
}

#[derive(Debug, Deserialize)]
pub struct MediaConfig {
    pub music_path: PathBuf,
    pub video_path: PathBuf,
    pub filesharing_path: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        let config_str = fs::read_to_string("config.toml")?;
        let config: Config = toml::from_str(&config_str)?;

        Ok(config)
    }
}

pub fn get_config() -> Result<&'static Config, Error> {
    static CONFIG: OnceLock<Result<Config, String>> = OnceLock::new();

    let result = CONFIG.get_or_init(|| {
        match Config::load() {
            Ok(config) => Ok(config),
            Err(err) => Err(err.to_string())
        }
    });

    match result {
        Ok(config) => Ok(config),
        Err(err) => Err(anyhow!("{}", err))
    }
}