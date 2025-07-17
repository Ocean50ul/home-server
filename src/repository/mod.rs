pub mod artists_repo;
pub mod albums_repo;
pub mod tracks_repo;

pub use artists_repo::SqliteArtistsRepository;
pub use albums_repo::SqliteAlbumsRepository;
pub use tracks_repo::SqliteTracksRepository;

use artists_repo::ArtistConversionError;
use albums_repo::AlbumConversionError;
use tracks_repo::TrackConversionError;
use crate::domain::UploadedParseError;

use uuid::Uuid;
use std::path::PathBuf;

/* Database related errors */
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    // this stuff is thrown by track_repo::by_path_fetch and track_repo::path_exists
    // when you converting PathBuf into string
    #[error("Path contains non-UTF8 characters: {0:?}")]
    InvalidPathEncoding(PathBuf),

    // this stuff is thrown by delete_by_id functions.
    #[error("Item with id <{0}> was not found.")]
    IdNotFound(Uuid),

    // this stuff is thrown by exists functions, if DB for some reason returns something beside 0 or 1
    #[error("Unknown Error occured! Have fun debugging it, here is something to help you: {0}")]
    UnknownError(String),

    // this stuff is thrown by save_all functions since you need to "manually" parse bytes into uuid
    // perhaps its not a good design of a function
    #[error("Error retrieving Uuid bytes from DB. Expected 16, found {0}.")]
    InvalidUuidLength(usize),

    #[error("Integer conversion error: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),

    #[error("Uuid conversion error: {0}")]
    UuidConversion(#[from] uuid::Error),

    #[error("Data mapping error for Artist: {0}")]
    ArtistDataMapping(#[from] ArtistConversionError),

    #[error("Data mapping error for Album: {0}")]
    AlbumDataMapping(#[from] AlbumConversionError),

    #[error("Data mapping error for Track: {0}")]
    TrackDataMapping(#[from] TrackConversionError),

    #[error("Uploaded conversion error: {0}")]
    UploadedConversion(#[from] UploadedParseError),

    #[error("No rows was returned by a query that expected to return at least one row.")]
    RowNotFound,

    #[error("Database connection error: {0}")]
    ConnectionError(String),

    #[error("Something went wrong, dude, idk what, look at this: {0}")]
    GenericDatabaseError(#[from] sqlx::Error),

    #[error("A constraint was violated: {description}")]
    ConstraintViolation { description: String },

    #[error("Failed to decode database row: {0}")]
    RowDecodingError(String),

    #[error("Failed to get column data")]
    ColumnGetError
}

impl RepositoryError {
    pub fn from_sqlx_error(sqlx_error: sqlx::Error) -> Self {
        match &sqlx_error {
            sqlx::Error::RowNotFound => Self::RowNotFound,
            sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::Tls(_) => Self::ConnectionError(sqlx_error.to_string()),
            sqlx::Error::Decode(decode_err) => Self::RowDecodingError(decode_err.to_string()),
            sqlx::Error::Database(db_error) => {
                if let Some(error_code) = db_error.code() {
                    let code_str = error_code.as_ref();

                    // SQLite specific error codes for constraints
                    // 19: General constraint violation (SQLITE_CONSTRAINT)
                    // 2067: SQLITE_CONSTRAINT_UNIQUE (specific unique constraint violation)
                    // 1555: SQLITE_CONSTRAINT_PRIMARYKEY (specific primary key violation)
                    // 787: SQLITE_CONSTRAINT_FOREIGNKEY (specific foreign key violation)
                    if ["19", "2067", "1555", "787"].contains(&code_str) {
                        return Self::ConstraintViolation {
                            description: db_error.message().to_string()
                        };
                    }
                }

                Self::GenericDatabaseError(sqlx_error)
            },

            _ => Self::GenericDatabaseError(sqlx_error)
        }
    }
}

/* Helper trait for id parameter of repository functions */
pub trait IntoUuid {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError>;
}

impl IntoUuid for Uuid {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError> {
        Ok(*self)
    }
}

impl IntoUuid for &Uuid {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError> {
        Ok(**self)
    }
}

impl IntoUuid for &str {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError> {
        Uuid::parse_str(self).map_err(RepositoryError::UuidConversion)
    }
}

impl IntoUuid for String {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError> {
        Uuid::parse_str(&self).map_err(RepositoryError::UuidConversion)
    }
}

impl IntoUuid for &String {
    fn into_uuid(&self) -> Result<Uuid, RepositoryError> {
        Uuid::parse_str(&self).map_err(RepositoryError::UuidConversion)
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {

    use sqlx::{SqlitePool, Error as SqlxError};

    use crate::domain::ValidationError;
    use super::RepositoryError;

    #[derive(Debug, thiserror::Error)]
    pub enum TestSetupError {
        #[error("Database operation failed: {0}")]
        DbError(#[from] sqlx::Error),

        #[error("Repository operation failed: {0}")]
        RepositoryError(#[from] RepositoryError),

        #[error("Entity fields validation failed: {0}")]
        FieldsValidationError(#[from] ValidationError),

        #[error("Stream has returned unexpected error: {0}")]
        StreamError(RepositoryError),
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
}