// DEPRICATED
// Pre-refactored monolith version of repositories. 1000 loc without test, a bit too much imo. 

use sqlx::{Executor, FromRow, QueryBuilder, Row, Sqlite, SqliteConnection};
use futures::{Stream, StreamExt};
use uuid::Uuid;
use std::{fmt::Debug, path::{Path, PathBuf}};
use chrono::NaiveDateTime;

use crate::models::music::{
    Album, Artist, BatchDeleteResult, BatchSaveResult, Track, Uploaded, UploadedParseError, AudioFileType, AudioFileTypeError
};

#[derive(FromRow)]
struct DbTrack {
    id: Vec<u8>,
    name: String,
    album_id: Vec<u8>,
    duration: i64,
    file_path: String,
    file_size: i64,
    file_type: String,
    uploaded: String,
    date_added: Option<NaiveDateTime>
}

impl TryFrom<DbTrack> for Track {
    type Error = RepositoryError;
    fn try_from(db_track: DbTrack) -> Result<Self, Self::Error> {
        Ok(
            Self {
                id: Uuid::from_slice(&db_track.id)?,
                name: db_track.name,
                album_id: Uuid::from_slice(&db_track.album_id)?,
                duration: u32::try_from(db_track.duration)?,
                file_path: db_track.file_path,
                file_size: u64::try_from(db_track.file_size)?,
                file_type: AudioFileType::from_extension_str(&db_track.file_type),
                uploaded: db_track.uploaded.try_into()?,
                date_added: db_track.date_added,
            }
        )
    }
}

#[derive(FromRow)]
struct DbAlbum {
    id: Vec<u8>,
    name: String,
    artist_id: Vec<u8>,
    year: Option<i64>
}

impl TryFrom<DbAlbum> for Album {
    type Error = RepositoryError;
    fn try_from(db_album: DbAlbum) -> Result<Self, Self::Error> {
        Ok(
            Self {
                id: Uuid::from_slice(&db_album.id)?,
                name: db_album.name,
                artist_id: Uuid::from_slice(&db_album.artist_id)?,
                year: db_album.year
            }
        )
    }
}

#[derive(FromRow)]
struct DbArtist {
    id: Vec<u8>,
    name: String
}

impl TryFrom<DbArtist> for Artist {
    type Error = RepositoryError;
    fn try_from(db_artist: DbArtist) -> Result<Self, Self::Error> {
        Ok(
            Self {
                id: Uuid::from_slice(&db_artist.id)?,
                name: db_artist.name
            }
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error("Path contains non-UTF8 characters: {0:?}")]
    InvalidPathEncoding(PathBuf),

    #[error("Item not found with id: {0}")]
    NotFound(String),

    #[error("Unknown Error occured! Have fun debugging it, here is something to help you: {0}")]
    UnknownError(String),

    #[error("Error retrieving Uuid bytes from DB. Expected 16, found {0}.")]
    InvalidUuidLength(usize),

    // valid 
    #[error("Integer conversion error: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),

    #[error("Uuid conversion error: {0}")]
    UuidConversion(#[from] uuid::Error),

    #[error("Uploaded conversion error: {0}")]
    UploadedConversion(#[from] UploadedParseError),
    // valid ^^

    #[error("File type conversion error: {0}")]
    FileTypeError(#[from] AudioFileTypeError)

}

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



/***********************************************************************************************
 *                                      Tracks Repository                                      *
*==============================================================================================*/
pub struct SqliteTracksRepository;

impl SqliteTracksRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteTracksRepository {

    pub async fn save<'e, E>(&self, executor: E, track: &Track) -> Result<Track, RepositoryError>
    where E: Executor<'e, Database = Sqlite>,
    {
        let uploaded_str: &str = track.uploaded.into();

        let db_track = sqlx::query_as::<_, DbTrack>(
            "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added;")
            .bind(&track.id)
            .bind(&track.name)
            .bind(&track.album_id)
            .bind(&track.duration)
            .bind(&track.file_path)
            .bind(track.file_size as i64)
            .bind(&track.file_type.as_str())
            .bind(&uploaded_str)
            .bind(&track.date_added)
            .fetch_one(executor)
            .await?;

        Ok(db_track.try_into()?)
    }

    pub async fn save_all(&self, tx: &mut SqliteConnection, tracks: &[Track]) -> Result<Vec<Uuid>, RepositoryError> {
        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) "
        );

        qbuilder.push_values(tracks.iter(), |mut b, track| {
            let uploaded_str: &str = track.uploaded.into();

            b.push_bind(&track.id)
                .push_bind(&track.name)
                .push_bind(&track.album_id)
                .push_bind(&track.duration)
                .push_bind(&track.file_path)
                .push_bind(track.file_size as i64)
                .push_bind(track.file_type.as_str())
                .push_bind(uploaded_str)
                .push_bind(&track.date_added);
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(tx).await?;
        rows.into_iter()
            .map(|row| {
                let bytes: Vec<u8> = row.try_get(0)?;
                if bytes.len() != 16 {
                    return Err(RepositoryError::InvalidUuidLength(bytes.len()));
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                Ok(Uuid::from_bytes(arr))
            })
            .collect()
    }

    pub async fn prw_save_all<'e, 'a, E>(&self, executor: &'a E, tracks: &[Track]) -> Result<BatchSaveResult, RepositoryError>
    where &'a E: Executor<'e, Database = Sqlite>
    {
        // This is per row insert, which is not optimized since there n = tracks.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which track failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass.

        let mut batch_result = BatchSaveResult::new(tracks.len());

        for (index, track) in tracks.iter().enumerate() {
            let uploaded_str: &str = track.uploaded.into();

            let q_result = sqlx::query(
                "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                RETURNING id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added;")
                .bind(&track.id)
                .bind(&track.name)
                .bind(&track.album_id)
                .bind(&track.duration)
                .bind(&track.file_path)
                .bind(track.file_size as i64)
                .bind(track.file_type.as_str())
                .bind(&uploaded_str)
                .bind(&track.date_added)
                .execute(executor)
                .await;

            match q_result {
                Ok(_) => batch_result.saved.push((index, track.id)),
                Err(err) => {
                    match err {
                        sqlx::Error::Database(err) => {
                            if err.is_unique_violation() {
                                batch_result.skipped.push(index);
                            } else {
                                batch_result.failed.push((index, RepositoryError::Sqlx(sqlx::Error::Database(err))));
                            }
                        },
                        _ => batch_result.failed.push((index, RepositoryError::Sqlx(err))),
                    }
                }
            }
        }

        Ok(batch_result)
    }

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Track>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let uuid = id.into_uuid()?;
        let db_track = sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks 
            WHERE id = ? 
            LIMIT 1;"
        )
        .bind(uuid)
        .fetch_optional(executor)
        .await?;

        db_track.map(Track::try_from)
            .transpose()
    }

    pub async fn by_path_fetch<'e, E, P>(&self, executor: E, path: P) -> Result<Option<Track>, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        P: AsRef<Path> + Send + Sync + 'static
    {
        let path_ref = path.as_ref();
        if let Some(path_str) = path_ref.to_str() {
            let db_track = sqlx::query_as::<_, DbTrack>(
                "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
                FROM tracks 
                WHERE file_path = ? 
                LIMIT 1;"
            )
            .bind(path_str)
            .fetch_optional(executor)
            .await?;
    
            return db_track.map(Track::try_from)
                .transpose();
        }

        Err(RepositoryError::InvalidPathEncoding(path_ref.to_path_buf()))
        
    }

    pub async fn stream_all<'e, E>(&self, executor: E) -> impl Stream<Item = Result<Track, RepositoryError>> + Send + 'e
    where 
        E: Executor<'e, Database = Sqlite> + Send + 'e,
        Track: Send,
        RepositoryError: Send
    {
        sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks"
        )
        .fetch(executor)
        .map(|db_track_res|{
            match db_track_res {
                Ok(db_track) => Track::try_from(db_track),
                Err(sqlx_err) => Err(RepositoryError::Sqlx(sqlx_err))
            }
        })
    }

    pub async fn all_by_album<'e, E, ID>(&self, executor: E, album_id: ID) -> Result<Vec<Track>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let album_id = album_id.into_uuid()?;

        let db_tracks = sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks
            WHERE album_id = ?"
        ).bind(album_id)
        .fetch_all(executor)
        .await?;
        
        db_tracks
            .into_iter()
            .map(Track::try_from)
            .collect()
    }

    pub async fn stream_by_uploaded<'e, E>(&self, executor: E, uploaded_by: Uploaded) -> impl Stream<Item = Result<Track, RepositoryError>> + Send + 'e
    where 
        E: Executor<'e, Database = Sqlite> +'e,
        Track: Send,
        RepositoryError: Send
    {   
        let uploaded_str: &str = uploaded_by.into();
        sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks
            WHERE uploaded = ?"
        ).bind(uploaded_str)
        .fetch(executor)
        .map(|db_track_res|{
            match db_track_res {
                Ok(db_track) => Track::try_from(db_track),
                Err(sqlx_err) => Err(RepositoryError::Sqlx(sqlx_err))
            }
        })
    }
    
    pub async fn delete<ID>(&self, executor: &mut SqliteConnection, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let result = sqlx::query(
            "DELETE FROM tracks WHERE tracks.id = ?;",
        ).bind(id)
        .execute(executor)
        .await?;

        if result.rows_affected() > 0 {
            Ok(())
        } else {
            Err(RepositoryError::NotFound(id.to_string()))
        }
    }

    pub async fn prw_delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<BatchDeleteResult, RepositoryError> 
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        // This is per row DELETE, which is not optimized since there n = ids.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly DELETE items of the batch 
        // that are valid and to report in detailes which entity has failed to be deleted and why. There is
        // a way to DELETE everything in one query, but then either granuality of errors report
        // will suffer or batch DELETE become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass. And just in case i've made regular delete_all function.

        let mut batch_result = BatchDeleteResult::new(ids.len());

        for id in ids {
            let uuid = id.into_uuid()?;
            let delete_result = self.delete(executor, uuid).await;
            match delete_result {
                Ok(_) => batch_result.deleted_ids.push(uuid.clone()),
                Err(err) => batch_result.failed.push((uuid.clone(), err))
            }

        }

        Ok(batch_result)
    }

    pub async fn delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<u64, RepositoryError>
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        let mut qbuilder = QueryBuilder::new(
            "DELETE FROM tracks WHERE id IN ("
        );
        let mut separated = qbuilder.separated(", ");
        for id in ids.iter() {
            let uuid = id.into_uuid()?;
            separated.push_bind(uuid);
        }
        separated.push_unseparated(");");

        let query = qbuilder.build();
        let result = query.execute(executor).await?;

        Ok(result.rows_affected())
    }
    
    pub async fn id_exists<'exec, E, ID>(&self, executor: E, id: ID) -> Result<bool, RepositoryError>
    where 
        E: Executor<'exec, Database = Sqlite> + Send,
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE tracks.id = ? LIMIT 1);",
            id
        )
        .fetch_one(executor)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => {
                let err_string = format!("Unexpected value returned from EXISTS query for ID {}: {}", id.to_string(), somethingelse);
                Err(RepositoryError::UnknownError(err_string))
            }
        }
    }

    pub async fn path_exists<'e, E, P>(&self, executor: E, path: P) -> Result<bool, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        P: AsRef<Path> + Send + Sync + 'static
    {
        let path_str = path.as_ref().to_str();
        match path_str {
            Some(pstr) => {
                let the_answer = sqlx::query_scalar!(
                    "SELECT EXISTS(SELECT 1 FROM tracks WHERE tracks.file_path = ? LIMIT 1);",
                    path_str
                )
                .fetch_one(executor)
                .await?;
        
                match the_answer {
                    0 => Ok(false),
                    1 => Ok(true),
                    somethingelse => {
                        let err_string = format!("Unexpected value returned from EXISTS query for path {}: {}", pstr, somethingelse);
                        Err(RepositoryError::UnknownError(err_string))
                    }
                }
            },
            None => Err(RepositoryError::InvalidPathEncoding(path.as_ref().to_path_buf()))
        }
    }
        
}


/***********************************************************************************************
 *                                         Albums Repository                                   *
*==============================================================================================*/
pub struct SqliteAlbumsRepository;

impl SqliteAlbumsRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteAlbumsRepository {
    pub async fn save<'e, E>(&self, executor: E, album: &Album) -> Result<Album, RepositoryError>
    where E: Executor<'e, Database = Sqlite>
    {
        let db_album = sqlx::query_as::<_, DbAlbum>(
            "INSERT INTO albums(id, name, artist_id, year) 
            VALUES (?, ?, ?, ?)
            RETURNING *;"
        )
        .bind(&album.id)
        .bind(&album.name)
        .bind(&album.artist_id)
        .bind(album.year)
        .fetch_one(executor)
        .await?;

        Ok(db_album.try_into()?)
    }

    pub async fn save_all(&self, tx: &mut SqliteConnection, albums: &[Album]) -> Result<Vec<Uuid>, RepositoryError> {
        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO albums(id, name, artist_id, year) "
        );

        qbuilder.push_values(albums.iter(), |mut b, album| {
            b.push_bind(&album.id)
                .push_bind(&album.name)
                .push_bind(&album.artist_id)
                .push_bind(album.year);
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(tx).await?;
        rows.into_iter()
            .map(|row| {
                let bytes: Vec<u8> = row.try_get(0)?;
                if bytes.len() != 16 {
                    return Err(RepositoryError::InvalidUuidLength(bytes.len()));
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                Ok(Uuid::from_bytes(arr))
            })
            .collect()
    }

    pub async fn prw_save_all<'e, 'a, E>(&self, executor: &'a E, albums: &[Album]) -> BatchSaveResult
    where &'a E: Executor<'e, Database = Sqlite>
    {
        // This is per row insert, which is not optimized since there n = albums.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which album failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass.

        let mut batch_result = BatchSaveResult::new(albums.len());

        for (index, album) in albums.iter().enumerate() {

            let q_result = sqlx::query(
                "INSERT INTO albums(id, name, artist_id, year)
                VALUES (?, ?, ?, ?)
                RETURNING id, name, artist_id, year;")
                .bind(&album.id)
                .bind(&album.name)
                .bind(&album.artist_id)
                .bind(album.year)
                .execute(executor)
                .await;

                match q_result {
                    Ok(_) => batch_result.saved.push((index, album.id)),
                    Err(err) => {
                        match err {
                            sqlx::Error::Database(err) => {
                                if err.is_unique_violation() {
                                    batch_result.skipped.push(index);
                                } else {
                                    batch_result.failed.push((index, RepositoryError::Sqlx(sqlx::Error::Database(err))));
                                }
                            },
                            _ => batch_result.failed.push((index, RepositoryError::Sqlx(err))),
                        }
                    }
                }
        }

        batch_result
    }

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Album>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let db_album = sqlx::query_as::<_, DbAlbum>(
            "SELECT * FROM albums WHERE id = ? LIMIT 1;"
        )
        .bind(id)
        .fetch_optional(executor)
        .await?;

        db_album.map(Album::try_from)
            .transpose()
    }
    
    pub async fn stream_all<'e, E>(&self, executor: E) -> impl Stream<Item = Result<Album, RepositoryError>> + 'e
    where E: Executor<'e, Database = Sqlite> + 'e
    {
        sqlx::query_as::<_, DbAlbum>(
            "SELECT * FROM albums;"
        )
        .fetch(executor)
        .map(|db_alb_result|{
            match db_alb_result {
                Ok(db_alb) => Album::try_from(db_alb),
                Err(err) => Err(RepositoryError::Sqlx(err))
            }
        })
    }

    pub async fn all_by_artist<'e, E, ID>(&self, executor: E, artist_id: ID) -> Result<Vec<Album>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let artist_id = artist_id.into_uuid()?;
        let db_albums = sqlx::query_as::<_, DbAlbum>(
            "SELECT id, name, artist_id, year
            FROM albums
            WHERE artist_id = ?"
        ).bind(artist_id)
        .fetch_all(executor)
        .await?;

        db_albums
            .into_iter()
            .map(Album::try_from)
            .collect()
    }
    
    pub async fn delete<ID>(&self, executor: &mut SqliteConnection, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let result = sqlx::query(
            "DELETE FROM albums WHERE albums.id = ?;",
        ).bind(id)
        .execute(executor)
        .await?;

        if result.rows_affected() > 0 {
            Ok(())
        } else {
            Err(RepositoryError::NotFound(id.to_string()))
        }
    }

    pub async fn prw_delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<BatchDeleteResult, RepositoryError> 
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        // This is per row DELETE, which is not optimized since there n = ids.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly DELETE items of the batch 
        // that are valid and to report in detailes which entity has failed to be deleted and why. There is
        // a way to DELETE everything in one query, but then either granuality of errors report
        // will suffer or batch DELETE become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass. And just in case i've made regular delete_all function.

        let mut batch_result = BatchDeleteResult::new(ids.len());

        for id in ids {
            let uuid = id.into_uuid()?;
            let delete_result = self.delete(executor, uuid).await;
            match delete_result {
                Ok(_) => batch_result.deleted_ids.push(uuid.clone()),
                Err(err) => batch_result.failed.push((uuid.clone(), err))
            }

        }

        Ok(batch_result)
    }

    pub async fn delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<u64, RepositoryError>
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        let mut qbuilder = QueryBuilder::new(
            "DELETE FROM albums WHERE id IN ("
        );
        let mut separated = qbuilder.separated(", ");
        for id in ids.iter() {
            let uuid = id.into_uuid()?;
            separated.push_bind(uuid);
        }
        separated.push_unseparated(");");

        let query = qbuilder.build();
        let result = query.execute(executor).await?;

        Ok(result.rows_affected())
    }
    
    pub async fn id_exists<'e, E, ID>(&self, executor: E, id: ID) -> Result<bool, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE albums.id = ? LIMIT 1);",
            id
        )
        .fetch_one(executor)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => {
                let err_string = format!("Unexpected value returned from EXISTS query for ID {}: {}", id.to_string(), somethingelse);
                Err(RepositoryError::UnknownError(err_string))
            }
        }
    }
}

/***********************************************************************************************
 *                                      Artists Repository                                     *
*==============================================================================================*/
pub struct SqliteArtistsRepository;

impl SqliteArtistsRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteArtistsRepository {
    pub async fn save<'e, E>(&self, executor: E, artist: &Artist) -> Result<Artist, RepositoryError>
    where E: Executor<'e, Database = Sqlite>
    {   
        let db_artist = sqlx::query_as::<_, DbArtist>(
            "INSERT INTO artists(id, name) 
            VALUES (?, ?)
            RETURNING *;")
            .bind(&artist.id)
            .bind(&artist.name)
            .fetch_one(executor)
            .await?;

        Ok(db_artist.try_into()?)
    }

    pub async fn save_all(&self, tx: &mut SqliteConnection, artists: &[Artist]) -> Result<Vec<Uuid>, RepositoryError> {
        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO artists(id, name) "
        );

        qbuilder.push_values(artists.iter(), |mut b, artist| {
            b.push_bind(&artist.id)
                .push_bind(&artist.name);
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(tx).await?;
        rows.into_iter()
            .map(|row| {
                let bytes: Vec<u8> = row.try_get(0)?;
                if bytes.len() != 16 {
                    return Err(RepositoryError::InvalidUuidLength(bytes.len()));
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                Ok(Uuid::from_bytes(arr))
            })
            .collect()
    }

    pub async fn prw_save_all<'e, 'a, E>(&self, executor: &'a E, artists: &[Artist]) -> BatchSaveResult
    where &'a E: Executor<'e, Database = Sqlite>
    {
        // This is per row insert, which is not optimized since there n = artists.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass. And just in case i've made regular save_all function.

        let mut batch_result = BatchSaveResult::new(artists.len());

        for (index, artist) in artists.iter().enumerate() {
            let q_result = sqlx::query(
                "INSERT INTO artists(id, name)
                VALUES (?, ?);"
            )
            .bind(&artist.id)
            .bind(&artist.name)
            .execute(executor)
            .await;

            match q_result {
                Ok(_) => batch_result.saved.push((index, artist.id)),
                Err(err) => {
                    match err {
                        sqlx::Error::Database(err) => {
                            if err.is_unique_violation() {
                                batch_result.skipped.push(index);
                            } else {
                                batch_result.failed.push((index, RepositoryError::Sqlx(sqlx::Error::Database(err))));
                            }
                        },
                        _ => batch_result.failed.push((index, RepositoryError::Sqlx(err))),
                    }
                }
            }
        }

        batch_result
    }

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Artist>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let db_artist = sqlx::query_as::<_, DbArtist>(
            "SELECT * FROM artists WHERE id = ? LIMIT 1;"
        )
        .bind(id)
        .fetch_optional(executor)
        .await?;

        db_artist.map(Artist::try_from)
            .transpose()
    }
    
    pub async fn stream_all<'e, E>(&self, executor: E) -> impl Stream<Item = Result<Artist, RepositoryError>> +'e
    where E: Executor<'e, Database = Sqlite> +'e
    {
        sqlx::query_as::<_, DbArtist>(
            "SELECT * FROM artists;"
        )
        .fetch(executor)
        .map(|db_art_res|{
            match db_art_res {
                Ok(db_artist) => Artist::try_from(db_artist),
                Err(err) => Err(RepositoryError::Sqlx(err))
            }
        })
    }
    
    pub async fn delete<ID>(&self, executor: &mut SqliteConnection, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let result = sqlx::query(
            "DELETE FROM artists WHERE artists.id = ?;",
        ).bind(id)
        .execute(executor)
        .await?;

        if result.rows_affected() > 0 {
            Ok(())
        } else {
            Err(RepositoryError::NotFound(id.to_string()))
        }
    }

    pub async fn prw_delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<BatchDeleteResult, RepositoryError> 
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        // This is per row DELETE, which is not optimized since there n = ids.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly DELETE items of the batch 
        // that are valid and to report in detailes which entity has failed to be deleted and why. There is
        // a way to DELETE everything in one query, but then either granuality of errors report
        // will suffer or batch DELETE become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass. And just in case i've made regular delete_all function.

        let mut batch_result = BatchDeleteResult::new(ids.len());

        for id in ids {
            let uuid = id.into_uuid()?;
            let delete_result = self.delete(executor, uuid).await;
            match delete_result {
                Ok(_) => batch_result.deleted_ids.push(uuid),
                Err(err) => batch_result.failed.push((uuid, err))
            }

        }

        Ok(batch_result)
    }

    pub async fn delete_all<'e, ID>(&self, executor: &mut SqliteConnection, ids: &'e [ID]) -> Result<u64, RepositoryError>
    where 
        ID: IntoUuid + Send + Sync + 'static,
    {
        let mut qbuilder = QueryBuilder::new(
            "DELETE FROM artists WHERE id IN ("
        );
        let mut separated = qbuilder.separated(", ");
        for id in ids.iter() {
            let uuid = id.into_uuid()?;
            separated.push_bind(uuid);
        }
        separated.push_unseparated(");");

        let query = qbuilder.build();
        let result = query.execute(executor).await?;

        Ok(result.rows_affected())
    }
    
    pub async fn id_exists<'e, ID, E>(&self, executor: E, id: ID) -> Result<bool, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync + 'static
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM artists WHERE artists.id = ? LIMIT 1);",
            id
        )
        .fetch_one(executor)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => {
                let err_string = format!("Unexpected value returned from EXISTS query for ID {}: {}", id.to_string(), somethingelse);
                Err(RepositoryError::UnknownError(err_string))
            }
        }
    }
}
