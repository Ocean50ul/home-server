pub mod track;
pub mod album;
pub mod artist;
pub mod uploaded;
pub mod audiofile;

use std::ffi::OsStr;
use serde::{Serialize, Deserialize};
use thiserror;
use uuid::Uuid;
use lofty::file::FileType as LoftyFileType;

use crate::repository::RepositoryError;

#[derive(Debug, thiserror::Error)]
#[error("Invalid 'uploaded' value: '{0}'. Expected 'masha' or 'denis'.")]
pub struct UploadedParseError(String);

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Name field cannot be an empty string.")]
    NameIsEmptyString,

    #[error("Duration cannot be zero.")]
    DurationIsZero,

    #[error("File size cannot be zero.")]
    FileSizeIsZero    
}

#[derive(Debug)]
pub struct BatchSaveOutcome {
    pub batch_index: usize,
    pub result: Result<Uuid, RepositoryError>
}

#[derive(Debug)]
pub struct BatchSaveReport {
    pub outcomes: Vec<BatchSaveOutcome>
}

impl BatchSaveReport
{
    pub fn new() -> Self {
        Self {
            outcomes: Vec::new()
        }
    }

    pub fn successful_ids(&self) -> Vec<Uuid> {
        self.outcomes.iter()
            .filter_map(|batch_outcome|{
                match batch_outcome.result {
                    Ok(id) => Some(id),
                    Err(_) => None
                }
            })
            .collect()
    }

    pub fn failed(&self) -> Vec<&BatchSaveOutcome> {
        self.outcomes.iter()
            .filter_map(|batch_outcome| {
                match batch_outcome.result {
                    Ok(_) => None,
                    Err(_) => Some(batch_outcome)
                }
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct BatchDeleteReport {
    pub deleted_ids: Vec<Uuid>,
    pub failed: Vec<(Uuid, RepositoryError)>
}

impl BatchDeleteReport {
    pub fn new() -> Self {
        Self {
            deleted_ids: Vec::new(),
            failed: Vec::new()
        }
    }
}