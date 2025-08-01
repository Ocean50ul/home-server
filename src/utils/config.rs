use serde::Deserialize;
use std::{fs, path::PathBuf};
use toml;
use std::sync::OnceLock;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigLoadingError {
    #[error("Failed to read the config (./config.toml): {0}")]
    FailedToReadConfig(String),

    #[error("Failed to parse the config: {0}")]
    FailedToParseConfig(#[from] toml::de::Error)
}

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
    pub ffmpeg_exe_path: PathBuf,
    pub ffmpeg_dir_path: PathBuf,
    pub ffmpeg_donwload_mirror: String,
    pub ffmpeg_sha_download_mirror: String,
    pub test_fixtures_path: PathBuf,
    pub resampled_music_path: PathBuf,
    pub audio_fixtures_json_path: PathBuf
}

impl Config {
    pub fn load() -> Result<Self, ConfigLoadingError> {
        let config_str = fs::read_to_string("config.toml").map_err(|err| ConfigLoadingError::FailedToReadConfig(err.to_string()))?;
        let config: Config = toml::from_str(&config_str)?;

        Ok(config)
    }
}

pub fn get_config() -> Result<&'static Config, ConfigLoadingError> {
    static CONFIG: OnceLock<Result<Config, ConfigLoadingError>> = OnceLock::new();

    let result = CONFIG.get_or_init(|| {
        Config::load()
    });

    match result {
        Ok(config) => Ok(config),
        Err(err) => Err(err.clone())
    }
}