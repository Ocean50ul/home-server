use std::fmt::Display;

use super::{UploadedParseError, Serialize, Deserialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Hash)]
pub enum Uploaded {
    Masha,
    Denis
}

impl TryFrom<String> for Uploaded {
    type Error = UploadedParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_lowercase().trim() {
            "masha" => Ok(Uploaded::Masha),
            "denis" => Ok(Uploaded::Denis),
            _ => Err(UploadedParseError(value)),
        }
    }
}

impl TryFrom<&str> for Uploaded {
    type Error = UploadedParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().trim() {
            "masha" => Ok(Uploaded::Masha),
            "denis" => Ok(Uploaded::Denis),
            _ => Err(UploadedParseError(value.to_string())),
        }
    }
}


impl From<Uploaded> for String {
    fn from(value: Uploaded) -> Self {
        match value {
            Uploaded::Denis => "denis".to_string(),
            Uploaded::Masha => "masha".to_string()
        }
    }
}

impl From<&Uploaded> for String {
    fn from(value: &Uploaded) -> Self {
        match value {
            &Uploaded::Denis => "denis".to_string(),
            &Uploaded::Masha => "masha".to_string()
        }
    }
}

impl From<Uploaded> for &str {
    fn from(value: Uploaded) -> Self {
        match value {
            Uploaded::Denis => "denis",
            Uploaded::Masha => "masha"
        }
    }
}

impl From<&Uploaded> for &str {
    fn from(value: &Uploaded) -> Self {
        match value {
            &Uploaded::Denis => "denis",
            &Uploaded::Masha => "masha"
        }
    }
}

impl Display for Uploaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            &Uploaded::Denis => write!(f, "denis"),
            &Uploaded::Masha => write!(f, "masha")
        }
    }
}