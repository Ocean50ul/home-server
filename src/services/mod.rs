pub mod scanner;
pub mod sync;
pub mod resample;
pub mod prepare;

use lofty::error::LoftyError;

use crate::domain::ValidationError;
use crate::repository::RepositoryError;

#[derive(Debug, thiserror::Error)]
pub enum SyncServiceError {
    #[error("Error while loading a config: {0}")]
    ConfigLoadingError(String),

    #[error(transparent)]
    RepositoryError(#[from] RepositoryError),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    IOError(#[from] std::io::Error),

    #[error("Lofty lib has failed to read the file: {0}")]
    FailedToReadAudioFile(#[from] LoftyError),

    #[error("Failed to exctract any metadata from a file: {0}")]
    FailedToExtractMetadata(String),

    #[error("Failed to exctract extension from a file {0}")]
    FailedToExtractExtension(String),

    #[error(transparent)]
    ScanError(#[from] ScanError),

    #[error("Validation error has occured: {0}")]
    DomainStructValidationError(#[from] ValidationError),
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("Walkdir error")]
    WalkdirError(#[from] walkdir::Error),

    #[error("Permission denied at {path}: {source}")]
    RootDirAccessError{path: String, source: std::io::Error},

    #[error(transparent)]
    IOError(#[from] std::io::Error)
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use std::{env::VarError, path::{Path, PathBuf}, sync::OnceLock};

    use log::SetLoggerError;
    use sqlx::{Error as SqlxError, SqlitePool};
    use tempfile::{NamedTempFile, Builder};

    use crate::{domain::{ValidationError}, repository::RepositoryError, services::{ScanError, SyncServiceError}, utils::{audio_fixtures::FixturesLoadingError}};

    pub const TEST_FIXTURES_JSON_PATH: &str = r"./audio_fixtures.json";
    
    #[derive(Debug, thiserror::Error)]
    pub enum TestSetupError {
        #[error("Failed to init env logger for the update_db tests: {0}")]
        LoggerError(String),

        #[error("I/O error: {0}")]
        IOError(#[from] std::io::Error),

        #[error("Sync Service error: {0}")]
        SyncServiceError(#[from] SyncServiceError),

        #[error("Scanner error: {0}")]
        ScannerError(#[from] ScanError),

        #[error("Wrong argument for a craete_temp_file function. DO NOT USE DOT!")]
        DotError(),

        #[error("Walker has encountered an error while walking test fixtures dir: {0}")]
        FixtureWalkerError(#[from] walkdir::Error),

        #[error("Invalid (non utf-8) test file name: {0}")]
        InvalidFixtureName(PathBuf),

        #[error("Error from a repository: {0}")]
        RepositoryError(#[from] RepositoryError),

        #[error("Validation error: {0}")]
        ValidationError(#[from] ValidationError),

        #[error("Error during setting up access tests: {0}")]
        SystemRootVariableNotFound(#[from] VarError),

        #[error(transparent)]
        FixturesLoadingError(#[from] FixturesLoadingError),

        #[error("Couldnt find fixture metadata: {0}")]
        FixtureMetadataDoesntExist(String)
    }

    pub async fn prepare_db() -> Result<SqlitePool, SqlxError> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await?;

        sqlx::migrate!(r".\data\db\migrations")
            .run(&pool)
            .await?;

        Ok(pool)      
    }

    pub fn init_logger() -> Result<(), TestSetupError> {
        static LOGGER_RESULT: OnceLock<Result<(), SetLoggerError>> = OnceLock::new();
    
        let init_result_ref = LOGGER_RESULT.get_or_init(|| {
            env_logger::builder()
                .is_test(true)
                .filter_level(log::LevelFilter::Warn)
                .try_init()
        });
    
        
        match init_result_ref {
            Ok(_) => Ok(()),
            Err(e) => Err(TestSetupError::LoggerError(e.to_string()))
        }
    
    }
    
    pub fn create_temp_files(path: &Path, amount: usize, ftype: &str) -> Result<Vec<NamedTempFile>, TestSetupError> {
    
        if ftype.contains(".") {
            return Err(TestSetupError::DotError());
        }
    
        (0..amount)
            .map(|i| {
                Builder::new()
                .prefix(&format!("{}_file_{}", ftype, i))
                .suffix(&format!(".{}", ftype))
                .tempfile_in(path)
                .map_err(TestSetupError::IOError)
            })
            .collect::<Result<Vec<NamedTempFile>, TestSetupError>>()
    
    }

    pub enum FixtureFileNames {
        FlacValidMetadata,
        Mp3CorruptedHeader,
        Mp3NoMetadata,
        Mp3ValidMetadata,
        WavValidMetadata,
        ChevelleClosure,
        ChevelleForfeit
    }

    impl FixtureFileNames {
        pub fn file_name(&self) -> String {
            match self {
                FixtureFileNames::FlacValidMetadata => "falc_valid_metadata.flac".to_string(),
                FixtureFileNames::Mp3ValidMetadata => "mp3_valid_metadata.mp3".to_string(),
                FixtureFileNames::WavValidMetadata => "wav_valid_metadata.wav".to_string(),

                FixtureFileNames::Mp3NoMetadata => "mp3_no_metadata.mp3".to_string(),
                FixtureFileNames::Mp3CorruptedHeader => "mp3_corrupted_header.mp3".to_string(),

                FixtureFileNames::ChevelleForfeit => "forfeit.flac".to_string(),
                FixtureFileNames::ChevelleClosure => "closure.mp3".to_string()
            }
        }
    }
}