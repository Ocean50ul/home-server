use std::{fmt::Debug, path::PathBuf};
use chrono::NaiveDateTime;

use crate::domain::audiofile::AudioFileType;
use crate::domain::uploaded::Uploaded;
use crate::utils::normalizations::{normalize_name, normalize_path};

use super::{ValidationError, Serialize, Deserialize, Uuid};

#[derive(Clone, Serialize, Deserialize, Hash, Debug)]
pub struct Track {
    id: Uuid,
    name: String,
    album_id: Uuid,
    duration: u32,
    file_path: PathBuf,
    file_size: u64,
    file_type: AudioFileType,
    uploaded: Uploaded,
    date_added: Option<NaiveDateTime>
}

impl AsRef<Track> for Track {
    fn as_ref(&self) -> &Track {
        self
    }
}

impl PartialEq for Track {
    fn eq(&self, other: &Self) -> bool {
        self.file_path() == other.file_path()
    }
}

impl Eq for Track {}

impl Track {

    pub fn new<S>(id: Uuid, name: S, album_id: Uuid, duration: u32, file_path: PathBuf, file_size: u64, file_type: AudioFileType, uploaded: Uploaded, date_added: Option<NaiveDateTime>) -> Result<Self, ValidationError> 
    where S: Into<String>
    {
        let norm_name = normalize_name(&name.into());
        let norm_path = normalize_path(&file_path);

        if norm_name.is_empty() { return Err(ValidationError::NameIsEmptyString); };
        if duration == 0 { return Err(ValidationError::DurationIsZero); };
        if file_size == 0 { return Err(ValidationError::FileSizeIsZero); };

        Ok(
            Self {
                id,
                name: norm_name,
                album_id,
                duration,
                file_path: norm_path,
                file_size,
                file_type,
                uploaded,
                date_added
            }
        )
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn album_id(&self) -> &Uuid {
        &self.album_id
    }

    pub fn duration(&self) -> u32 {
        self.duration
    }

    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn file_type(&self) -> &AudioFileType {
        &self.file_type
    }

    pub fn uploaded(&self) -> &Uploaded {
        &self.uploaded
    }

    pub fn date_added(&self) -> &Option<NaiveDateTime> {
        &self.date_added
    }
}