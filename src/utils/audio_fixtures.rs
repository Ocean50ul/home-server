use std::{fs::read_to_string, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{domain::audiofile::{AudioFileMetadata, AudioFileType}, utils::config::Config};

#[derive(Debug, thiserror::Error)]
pub enum FixturesLoadingError {
    #[error("IO error during tests fixtures loading: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Serialization error during tests fixtures loading: {0}")]
    SerializationError(#[from] serde_json::Error)
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioFixture {
    pub file_type: AudioFileType,
    pub file_name: PathBuf,

    pub metadata: AudioFileMetadata
}

impl AudioFixture {

}

pub fn load_fixtures(config: &Config) -> Result<Vec<AudioFixture>, FixturesLoadingError> {
    let json_string = read_to_string(&config.media.audio_fixtures_json_path)?;
    let fixtures = serde_json::from_str(&json_string)?;

    Ok(fixtures)
}