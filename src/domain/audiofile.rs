use std::path::PathBuf;

use lofty::{file::{AudioFile, TaggedFile, TaggedFileExt}, tag::Accessor};

use crate::utils::normalizations::normalize_name;
use super::{Serialize, Deserialize, OsStr, LoftyFileType};

#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize)]
pub enum AudioFileType {
    Flac,
    Mp3,
    Wav,
    Unknown
}

impl AudioFileType {

    pub fn from_lofty(lofty_type: &LoftyFileType) -> Self {
        match lofty_type {
            LoftyFileType::Flac => AudioFileType::Flac,
            LoftyFileType::Mpeg => AudioFileType::Mp3,
            LoftyFileType::Wav => AudioFileType::Wav,
            _other => AudioFileType::Unknown,
        }
    }

    pub fn from_extension_str(extension: &str) -> Self {
        match extension {
            "flac" => AudioFileType::Flac,
            "mp3" => AudioFileType::Mp3,
            "wav" => AudioFileType::Wav,
            _other => AudioFileType::Unknown
        }
    }

    pub fn from_os_ext(os_ext: &OsStr) -> Self {
        match os_ext.to_str() {
            Some(ext_str) => Self::from_extension_str(ext_str),
            None => AudioFileType::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AudioFileType::Flac => "flac",
            AudioFileType::Mp3 => "mp3",
            AudioFileType::Wav => "wav",
            AudioFileType::Unknown => "unknown"
        }
    }

    pub fn is_supported_extension(extension: &OsStr) -> bool {
        let ext_str = extension.to_string_lossy().to_lowercase();

        matches!(ext_str.as_str(), "flac" | "mp3" | "wav")
    }

    pub fn get_resample_target_rate(&self) -> u32 {
        match &self {
            &AudioFileType::Flac => 88200,
            &AudioFileType::Wav => 88200,
            &AudioFileType::Mp3 => 44100,
            _ => 44100
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioFileMetadata {
    pub artist_name: String,
    pub album_name: String,
    pub album_year: Option<u32>,

    pub track_name: String,
    pub track_duration: u32,
    pub sample_rate: Option<u32>
}

impl Default for AudioFileMetadata {
    fn default() -> Self {
        Self {
            artist_name: "unknown artist".to_string(),
            album_name: "unknown album".to_string(),
            album_year: None,
            track_name: "unknown track".to_string(),
            track_duration: 0,
            sample_rate: None
        }
    }
}

impl AudioFileMetadata {
    pub fn extract_or_default(tagged_result: Result<TaggedFile, lofty::error::LoftyError>) -> Self {
        match tagged_result {
            Ok(tagged) => Self::from_tagged(&tagged),
            Err(err) => {
                log::warn!("Could not read tags, using default metadata. Reason: {}", err);
                Self::default()
            }
        }
    }

    pub fn from_tagged(tagged_file: &TaggedFile) -> Self {
        let Some(lofty_tag) = tagged_file.primary_tag().or_else(|| tagged_file.first_tag()) else {
            return Self::default();
        };

       Self {
            artist_name: lofty_tag.artist().map_or_else(
                || normalize_name("unknown artist"),
                |s| normalize_name(&s)
            ),
            album_name: lofty_tag.album().map_or_else(
                || normalize_name("unknown album"),
                |s| normalize_name(&s)
            ),
            album_year: lofty_tag.year(),
            track_name: lofty_tag.title().map_or_else(
                || normalize_name("unknown track"),
                |s| normalize_name(&s)
            ),

            track_duration: tagged_file.properties().duration().as_secs().try_into().unwrap_or(0),
            sample_rate: tagged_file.properties().sample_rate()
       }
    }
}

#[derive(Debug, Clone)]
pub struct AudioFileDescriptor {
    pub path: PathBuf,
    pub file_size: u64,
    pub file_type: AudioFileType,
    pub metadata: AudioFileMetadata

    // TODO: cache
    // modified_time: SystemTime,
    // checksum: Option<u64>,
}