use super::{Uuid, ValidationError};
use crate::utils::normalizations::normalize_name;

#[derive(Clone, Debug)]
pub struct Artist {
    id: Uuid,
    name: String
}

impl AsRef<Artist> for Artist {
    fn as_ref(&self) -> &Artist {
        self
    }
}

impl PartialEq for Artist {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for Artist {}

impl Artist {

    pub fn new<S>(id: Uuid, name: S) -> Result<Self, ValidationError> 
    where S: Into<String>
    {
        let norm_name = normalize_name(&name.into());
        if norm_name.len() == 0 { return Err(ValidationError::NameIsEmptyString); }

        Ok(
            Self { id, name: norm_name }
        )
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name<S>(&mut self, name: S) -> Result<(), ValidationError> 
    where S: Into<String>
    {
        let norm_name = normalize_name(&name.into());
        if norm_name.len() == 0 { return Err(ValidationError::NameIsEmptyString); };
        self.name = norm_name;

        Ok(())
    }

    pub fn set_id(&mut self, id: Uuid) -> () {
        self.id = id
    }
}