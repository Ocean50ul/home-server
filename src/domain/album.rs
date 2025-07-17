use super::{Uuid, ValidationError};

use crate::utils::normalizations::normalize_name;

#[derive(Debug, Clone, Hash)]
pub struct Album {
    id: Uuid,
    name: String,
    artist_id: Uuid,
    year: Option<u32>
}

impl AsRef<Album> for Album {
    fn as_ref(&self) -> &Album {
        self
    }
}

impl PartialEq for Album {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name() && self.artist_id() == other.artist_id()
    }
}

impl Eq for Album {}

impl Album {

    pub fn new<S>(id: Uuid, name: S, artist_id: Uuid, year: Option<u32>) -> Result<Self, ValidationError> 
    where S: Into<String>
    {
        let norm_name = normalize_name(&name.into());
        if norm_name.len() == 0 { return Err(ValidationError::NameIsEmptyString); }

        Ok(
            Self { id, name: norm_name, artist_id, year }
        )
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn artist_id(&self) -> &Uuid {
        &self.artist_id
    }

    pub fn year(&self) -> Option<u32> {
        self.year
    }
}