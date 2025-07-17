use std::{convert::Infallible, path::{Path, PathBuf}, str::FromStr};

use futures::{Stream, StreamExt};
use sqlx::{Executor, FromRow, QueryBuilder, Row, Sqlite, SqliteConnection};
use chrono::NaiveDateTime;
use uuid::Uuid;

use crate::domain::{audiofile::AudioFileType, BatchDeleteReport, BatchSaveOutcome, BatchSaveReport, UploadedParseError, ValidationError};
use crate::domain::track::Track;
use crate::domain::uploaded::Uploaded;
use super::{IntoUuid, RepositoryError};

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
    type Error = TrackConversionError;
    fn try_from(db_track: DbTrack) -> Result<Self, Self::Error> {
        Ok(
            Self::new(
                Uuid::from_slice(&db_track.id)?,
                db_track.name,
                Uuid::from_slice(&db_track.album_id)?,
                u32::try_from(db_track.duration)?,
                PathBuf::from_str(&db_track.file_path).map_err(|err|TrackConversionError::PathStringConversionError(err))?,
                u64::try_from(db_track.file_size)?,
                AudioFileType::from_extension_str(&db_track.file_type),
                db_track.uploaded.try_into()?,
                db_track.date_added,
            ).map_err(|err| TrackConversionError::ValidationError(err))?
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrackConversionError {
    #[error("Uuid conversion error: {0}")]
    UuidConversionError(#[from] uuid::Error),

    #[error("Error during int conversion: {0}")]
    IntConversionError(#[from] std::num::TryFromIntError),

    #[error("Uuid conversion error: {0}")]
    UploadedConversionError(#[from] UploadedParseError),

    #[error("Error during path string conversion: {0}")]
    PathStringConversionError(Infallible),

    #[error("Error during validation of track fields: {0}")]
    ValidationError(#[from] ValidationError)
}

pub struct SqliteTracksRepository;

impl SqliteTracksRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteTracksRepository {

    pub async fn save<'e, E, T>(&self, executor: E, track: T) -> Result<Track, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        T: AsRef<Track> + Sync
    {
        let uploaded_str: &str = track.as_ref().uploaded().into();
        let file_path_str = track.as_ref().file_path().to_string_lossy();

        let db_track = sqlx::query_as::<_, DbTrack>(
            "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added;")
            .bind(&track.as_ref().id())
            .bind(&track.as_ref().name())
            .bind(&track.as_ref().album_id())
            .bind(&track.as_ref().duration())
            .bind(&file_path_str)
            .bind(track.as_ref().file_size() as i64)
            .bind(&track.as_ref().file_type().as_str())
            .bind(&uploaded_str)
            .bind(&track.as_ref().date_added())
            .fetch_one(executor)
            .await?;

        Ok(db_track.try_into()?)
    }

    pub async fn save_all<'e, E, T>(&self, executor: E, tracks: &[T]) -> Result<Vec<Uuid>, RepositoryError> 
    where 
        T: AsRef<Track> + Sync,
        E: Executor<'e, Database = Sqlite>
    {
        if tracks.is_empty() {
            return Ok(Vec::new());
        }

        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) "
        );

        qbuilder.push_values(tracks.iter(), |mut b, track| {
            let uploaded_str: &str = track.as_ref().uploaded().into();
            let file_path_str = track.as_ref().file_path().to_string_lossy();

            b.push_bind(track.as_ref().id())
                .push_bind(track.as_ref().name())
                .push_bind(track.as_ref().album_id())
                .push_bind(track.as_ref().duration())
                .push_bind(file_path_str)
                .push_bind(track.as_ref().file_size() as i64)
                .push_bind(track.as_ref().file_type().as_str())
                .push_bind(uploaded_str)
                .push_bind(track.as_ref().date_added());
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(executor).await?;
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

    pub async fn batch_save<T>(&self, connection: &mut SqliteConnection, tracks: &[T]) -> Result<BatchSaveReport, RepositoryError>
    where T: AsRef<Track> + Sync
    {
        // This is per row INSERT, so there is n = albums.len() queries.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why.
        // For some reasons, you cant do it in SQL. What a shame -- domain language my ass.

        let mut batch_report = BatchSaveReport::new();

        for (index, track) in tracks.iter().enumerate() {
            let track = track.as_ref();

            let id = track.id();
            let name = track.name();
            let ablum_id = track.album_id();
            let duration = track.duration();
            let uploaded_str: &str = track.uploaded().into();
            let file_size = track.file_size() as i64;
            let file_type = track.file_type().as_str();
            let file_path = track.file_path().to_string_lossy();
            let date_added = track.date_added();

            let saving_result = sqlx::query_scalar!(
                "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                RETURNING id;",
                id,
                name,
                ablum_id,
                duration,
                file_path,
                file_size,
                file_type,
                uploaded_str,
                date_added)
                .fetch_one(&mut *connection)
                .await
                .map_err(RepositoryError::from_sqlx_error)
                .and_then(|id_bytes| {
                    Uuid::from_slice(&id_bytes)
                        .map_err(RepositoryError::UuidConversion)
                    });
            
            batch_report.outcomes.push(
                BatchSaveOutcome {
                    batch_index: index,
                    result: saving_result
                }
            )

            
        }

        Ok(batch_report)
    }

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Track>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
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
        .await
        .map_err(RepositoryError::from_sqlx_error)?;

        db_track.map(Track::try_from)
            .transpose()
            .map_err(RepositoryError::TrackDataMapping)
    }

    pub async fn by_path_fetch<'e, E, P>(&self, executor: E, path: P) -> Result<Option<Track>, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        P: AsRef<Path> + Send + Sync
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
            .await
            .map_err(RepositoryError::from_sqlx_error)?;
    
            return db_track.map(Track::try_from)
                .transpose()
                .map_err(RepositoryError::TrackDataMapping);
        }

        Err(RepositoryError::InvalidPathEncoding(path_ref.to_path_buf()))
        
    }

    pub async fn stream_all<'e, E>(&self, executor: E) -> impl Stream<Item = Result<Track, RepositoryError>> + Send + 'e
    where 
        E: Executor<'e, Database = Sqlite> + Send + 'e,
    {
        sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks"
        )
        .fetch(executor)
        .map(|db_track_res|{
            match db_track_res {
                Ok(db_track) => Track::try_from(db_track).map_err(RepositoryError::TrackDataMapping),
                Err(sqlx_err) => Err(RepositoryError::from_sqlx_error(sqlx_err))
            }
        })
    }

    pub async fn all_by_album<'e, E, ID>(&self, executor: E, album_id: ID) -> Result<Vec<Track>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
    {
        let album_id = album_id.into_uuid()?;

        let db_tracks = sqlx::query_as::<_, DbTrack>(
            "SELECT id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added 
            FROM tracks
            WHERE album_id = ?"
        ).bind(album_id)
        .fetch_all(executor)
        .await
        .map_err(RepositoryError::from_sqlx_error)?;
        
        db_tracks
            .into_iter()
            .map(|db_track| Track::try_from(db_track).map_err(RepositoryError::TrackDataMapping))
            .collect()
    }

    pub async fn stream_by_uploaded<'e, E>(&self, executor: E, uploaded_by: Uploaded) -> impl Stream<Item = Result<Track, RepositoryError>> + Send + 'e
    where 
        E: Executor<'e, Database = Sqlite> +'e,
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
                Ok(db_track) => Track::try_from(db_track).map_err(RepositoryError::TrackDataMapping),
                Err(sqlx_err) => Err(RepositoryError::from_sqlx_error(sqlx_err))
            }
        })
    }
    
    pub async fn delete<'e, ID, E>(&self, executor: E, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync,
        E: Executor<'e, Database = Sqlite> +'e
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
            Err(RepositoryError::IdNotFound(id))
        }
    }

    pub async fn batch_delete<ID>(&self, connection: &mut SqliteConnection, ids: &[ID]) -> Result<BatchDeleteReport, RepositoryError> 
    where 
        ID: IntoUuid + Send + Sync
    {
        // This is per row DELETE, so there is n = ids.len() queries.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why.
        // For some reasons, you cant do it in SQL. What a shame -- domain language my ass.

        let mut batch_result = BatchDeleteReport::new();

        for id in ids {
            let uuid = id.into_uuid()?;
            let delete_result = self.delete(&mut *connection, uuid).await;
            match delete_result {
                Ok(_) => batch_result.deleted_ids.push(uuid.clone()),
                Err(err) => batch_result.failed.push((uuid.clone(), err))
            }

        }

        Ok(batch_result)
    }

    pub async fn delete_all<'e, ID, E>(&self, executor: E, ids: &'e [ID]) -> Result<u64, RepositoryError>
    where 
        ID: IntoUuid + Send + Sync,
        E: Executor<'e, Database = Sqlite>
    {
        if ids.is_empty() {
            return Ok(0);
        }

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
        ID: IntoUuid + Send + Sync
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE id = ? LIMIT 1);",
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
        P: AsRef<Path> + Send + Sync
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

#[cfg(test)]
mod tests {
    use std::fmt::Display;

    use chrono::Local;
    use sqlx::{SqlitePool, Transaction};

    use super::*;
    use crate::repository::{SqliteArtistsRepository, SqliteAlbumsRepository, test_helpers::{prepare_db, TestSetupError}};
    use crate::domain::{artist::Artist, album::Album};

    const UUID_BYTES: [u8; 16] = [
        0x0e, 0x81, 0x98, 0x2e,
        0xdf, 0x58, 0x47, 0xe9,
        0x8e, 0x7f, 0x3a, 0x2a,
        0xfb, 0x38, 0xb5, 0xcd,
    ]; // 0e81982e-df58-47e9-8e7f-3a2afb38b5cd

    const TRK_NAMESPACE: Uuid = Uuid::from_bytes(UUID_BYTES);

    struct TestContext {
        pool: SqlitePool,
        repo: SqliteTracksRepository,
        entities: Vec<Track>,

        art_repo: SqliteArtistsRepository,
        alb_repo: SqliteAlbumsRepository
    }

    impl TestContext {
        async fn new() -> Result<Self, TestSetupError> {
            let pool = prepare_db().await?;
            let art_repo = SqliteArtistsRepository::new();
            let alb_repo = SqliteAlbumsRepository::new();

            let artist = Artist::new(new_uuid("Default Artist"), "Default Artist Name")?;
            let album = Album::new(new_uuid("Default Album"), "Default Album Name", artist.id().clone(), Some(2042))?;

            art_repo.save(&pool, &artist).await?;
            alb_repo.save(&pool, &album).await?;
            
            Ok(
                Self {
                    pool,
                    repo: SqliteTracksRepository::new(),
                    entities: Vec::new(),
    
                    art_repo,
                    alb_repo,
                }
            )
        }

        async fn tx(&self) -> Result<Transaction<Sqlite>, TestSetupError> {
            self.pool.begin().await.map_err(TestSetupError::DbError)
        }

        fn with_entities(mut self, amount: u16) -> Result<Self, TestSetupError> {
            self.entities.extend(create_tracks(amount));

            Ok(self)
        }

        async fn associate<S>(&self, album_id: &S, artist_id: &S) -> Result<(), TestSetupError> 
        where S: AsRef<[u8]> + ?Sized + Display
        {
            let artist = Artist::new(new_uuid(artist_id), format!("Default Artist Name {}", artist_id))?;
            let album = Album::new(new_uuid(album_id), format!("Default Album Name {}", album_id), artist.id().clone(), Some(2042))?;

            self.art_repo.save(&self.pool, &artist).await?;
            self.alb_repo.save(&self.pool, &album).await?;

            Ok(())
        }
    }

    fn new_uuid<S>(name: &S) -> Uuid
    where S: AsRef<[u8]> + ?Sized
    {
        Uuid::new_v5(&TRK_NAMESPACE, name.as_ref())
    }

    fn create_tracks(amount: u16) -> Vec<Track> {
        (1..=amount)
            .map(|i| {
                let track_name= format!("Test Track #{}", i);
                let track_id = new_uuid(&track_name);
                let album_id = new_uuid("Default Album");

                Track::new(
                    track_id,
                    track_name,
                    album_id,
                    420 + i as u32,
                    PathBuf::from(format!("T:/stuff/{}/", i)),
                    49 + i as u64,
                    AudioFileType::Mp3,
                    Uploaded::Denis,
                    Some(Local::now().naive_local())
                ).expect("Error during test setup: album fields validation has failed.")
            })
            .collect()
    }

    fn create_tracks_with_album(amount: u16, album_id: Uuid) -> Vec<Track> {
        (1..=amount)
            .map(|i| {
                let track_name= format!("Test Track ##{}", i);
                let track_id = new_uuid(&track_name);

                Track::new(
                    track_id,
                    track_name,
                    album_id,
                    420 + i as u32,
                    PathBuf::from(format!("T:/stuff/more/{}/", i)),
                    49 + i as u64,
                    AudioFileType::Mp3,
                    Uploaded::Denis,
                    Some(Local::now().naive_local())
                ).expect("Error during test setup: album fields validation has failed.")
            })
            .collect()
    }


    #[tokio::test]
    async fn save_one_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        assert_eq!(saved_pool.id(), ctx.entities[0].id());
        assert_eq!(saved_pool.album_id(), ctx.entities[0].album_id());

        let mut tx = ctx.tx().await?;

        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        assert_eq!(saved_tx.id(), ctx.entities[1].id());
        assert_eq!(saved_tx.album_id(), ctx.entities[1].album_id());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn  save_one_failure() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;
        ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;

        let duplicate_pool_save = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await;
        assert!(duplicate_pool_save.is_err());

        let mut tx = ctx.tx().await?;
        ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;

        let duplicate_tx_save = ctx.repo.save(&mut *tx, &ctx.entities[1]).await;
        assert!(duplicate_tx_save.is_err());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn save_all_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let pool_chunk = &ctx.entities[0..10];
        let tx_chunk = &ctx.entities[10..20];

        let saved_pool_ids = ctx.repo.save_all(&ctx.pool, pool_chunk).await?;

        for album in pool_chunk {
            assert!(saved_pool_ids.contains(&album.id()));
        }

        let mut tx = ctx.tx().await?;
        let saved_tx_ids = ctx.repo.save_all(&mut *tx, tx_chunk).await?;
        
        for album in tx_chunk {
            assert!(saved_tx_ids.contains(&album.id()));
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn save_all_failure() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let pool_chunk = &ctx.entities[0..10];
        let tx_chunk = &ctx.entities[10..20];

        ctx.repo.save_all(&ctx.pool, &pool_chunk).await?;
        let duplicate_pool_save = ctx.repo.save_all(&ctx.pool, pool_chunk).await;

        assert!(duplicate_pool_save.is_err());

        let mut tx = ctx.tx().await?;
        ctx.repo.save_all(&mut *tx, &tx_chunk).await?;
        let duplicate_tx_save = ctx.repo.save_all(&mut *tx, tx_chunk).await;

        assert!(duplicate_tx_save.is_err());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn save_all_empty_vec() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(0)?;
        assert_eq!(ctx.entities.len(), 0);

        let saved_ids = ctx.repo.save_all(&ctx.pool, &ctx.entities).await?;
        assert_eq!(saved_ids.len(), 0);

        Ok(())
    }

    #[tokio::test]
    async fn batch_save_all_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let conn_chunk = &ctx.entities[0..10];
        let tx_chunk = &ctx.entities[10..20];

        let mut connection = ctx.pool.acquire().await?;
        let saved_conn_result = ctx.repo.batch_save(&mut connection, conn_chunk).await?;
        let saved_conn_ids = saved_conn_result.successful_ids();

        for entity in conn_chunk {
            assert!(saved_conn_ids.contains(&entity.id()))
        }

        let mut tx = ctx.tx().await?;
        let saved_tx_result = ctx.repo.batch_save(&mut *tx, tx_chunk).await?;
        let saved_tx_ids = saved_tx_result.successful_ids();

        for entity in tx_chunk {
            assert!(saved_tx_ids.contains(&entity.id()))
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn batch_save_all_failed() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let mut connection = ctx.pool.acquire().await?;
        let conn_chunk = &ctx.entities[0..10];
        let tx_chunk = &ctx.entities[10..20];

        ctx.repo.save_all(&ctx.pool, conn_chunk).await?;
        let pool_batch = ctx.repo.batch_save(&mut connection, conn_chunk).await?;
        assert_eq!(pool_batch.failed().len(), 10);

        ctx.repo.save_all(&ctx.pool, tx_chunk).await?;
        let mut tx = ctx.tx().await?;
        let tx_batch = ctx.repo.batch_save(&mut *tx, tx_chunk).await?;
        assert_eq!(tx_batch.failed().len(), 10);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn batch_save_mixed() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(40)?;
        let mut connection = ctx.pool.acquire().await?;
        let conn_chunk = &ctx.entities[0..20];
        let tx_chunk = &ctx.entities[20..40];

        ctx.repo.save_all(&ctx.pool, &conn_chunk[0..10]).await?;
        let pool_batch = ctx.repo.batch_save(&mut connection, conn_chunk).await?;
        assert_eq!(pool_batch.failed().len(), 10);
        assert_eq!(pool_batch.successful_ids().len(), 10);

        for entity in &conn_chunk[10..20] {
            assert!(pool_batch.successful_ids().contains(entity.id()))
        }

        let mut tx = ctx.tx().await?;
        ctx.repo.save_all(&mut *tx, &tx_chunk[0..10]).await?;
        let tx_batch = ctx.repo.batch_save(&mut *tx, tx_chunk).await?;

        assert_eq!(tx_batch.failed().len(), 10);
        assert_eq!(tx_batch.successful_ids().len(), 10);

        for entity in &tx_chunk[10..20] {
            assert!(tx_batch.successful_ids().contains(entity.id()))
        }

        Ok(())
    }

    #[tokio::test]
    async fn something_by_id_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_fetch_outcome = ctx.repo.by_id_fetch(&ctx.pool, saved_pool.id()).await?;

        assert!(pool_fetch_outcome.is_some());
        assert!(pool_fetch_outcome.unwrap().id() == ctx.entities[0].id());

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_fetch_outcome = ctx.repo.by_id_fetch(&mut *tx, saved_tx.id()).await?;

        assert!(tx_fetch_outcome.is_some());
        assert!(tx_fetch_outcome.unwrap().id() == ctx.entities[1].id());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn none_by_id_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_id = new_uuid("none by id fetch");

        let pool_fetch_outcome = ctx.repo.by_id_fetch(&ctx.pool, fake_id).await?;
        assert!(pool_fetch_outcome.is_none());

        let mut tx = ctx.tx().await?;
        let tx_fetch_outcome = ctx.repo.by_id_fetch(&mut *tx, fake_id).await?;
        assert!(tx_fetch_outcome.is_none());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn something_by_path_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_fetch_outcome = ctx.repo.by_path_fetch(&ctx.pool, saved_pool.file_path()).await?;

        assert!(pool_fetch_outcome.is_some());
        assert!(pool_fetch_outcome.unwrap().id() == ctx.entities[0].id());

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_fetch_outcome = ctx.repo.by_path_fetch(&mut *tx, saved_tx.file_path()).await?;

        assert!(tx_fetch_outcome.is_some());
        assert!(tx_fetch_outcome.unwrap().id() == ctx.entities[1].id());

        tx.commit().await?;


        Ok(())
    }

    #[tokio::test]
    async fn none_by_path_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_path = PathBuf::from("F:/fake/as/fuck");

        let pool_fetch_outcome = ctx.repo.by_path_fetch(&ctx.pool, &fake_path).await?;
        assert!(pool_fetch_outcome.is_none());

        let mut tx = ctx.tx().await?;
        let tx_fetch_outcome = ctx.repo.by_path_fetch(&mut *tx, &fake_path).await?;
        assert!(tx_fetch_outcome.is_none());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn stream_all_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(100)?;

        let saved_ids = ctx.repo.save_all(&ctx.pool, &ctx.entities).await?;

        let mut pool_stream = ctx.repo.stream_all(&ctx.pool).await;

        while let Some(track_result) = pool_stream.next().await {
            match track_result {
                Ok(track) => {
                    assert!(saved_ids.contains(&track.id()))
                },
                Err(err) => { return Err(TestSetupError::StreamError(err)) }
            }
        }

        let mut tx = ctx.tx().await?;

        {
            let mut tx_stream = ctx.repo.stream_all(&mut *tx).await;
            while let Some(track_result) = tx_stream.next().await {
                match track_result {
                    Ok(track) => {
                        assert!(saved_ids.contains(&track.id()))
                    },
                    Err(err) => { return Err(TestSetupError::StreamError(err)) }
                }
            }
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn stream_by_uploaded_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(100)?;

        let saved_ids = ctx.repo.save_all(&ctx.pool, &ctx.entities).await?;

        let mut pool_stream = ctx.repo.stream_by_uploaded(&ctx.pool, Uploaded::Denis).await;

        while let Some(track_result) = pool_stream.next().await {
            match track_result {
                Ok(track) => {
                    assert!(saved_ids.contains(&track.id()))
                },
                Err(err) => { return Err(TestSetupError::StreamError(err)) }
            }
        }

        let mut tx = ctx.tx().await?;

        {
            let mut tx_stream = ctx.repo.stream_all(&mut *tx).await;
            while let Some(track_result) = tx_stream.next().await {
                match track_result {
                    Ok(track) => {
                        assert!(saved_ids.contains(&track.id()))
                    },
                    Err(err) => { return Err(TestSetupError::StreamError(err)) }
                }
            }
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn all_by_album_something() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(10)?;
        ctx.associate("Newest Album", "Newest Artist").await?;

        let pool_chunk = &ctx.entities[0..10];
        let tx_chunk = create_tracks_with_album(10, new_uuid("Newest Album"));

        let pool_album_ids = ctx.repo.save_all(&ctx.pool, &pool_chunk).await?;

        let pool_fetched_albums = ctx.repo.all_by_album(&ctx.pool, new_uuid("Default Album")).await?;
        assert_eq!(pool_fetched_albums.len(), 10);

        for track in pool_fetched_albums {
            assert!(pool_album_ids.contains(track.id()));
        }

        let mut tx = ctx.tx().await?;
        let tx_album_ids = ctx.repo.save_all(&mut *tx, &tx_chunk).await?;

        let tx_fetched_albums = ctx.repo.all_by_album(&mut *tx, new_uuid("Newest Album")).await?;
        assert_eq!(tx_fetched_albums.len(), 10);

        for track in tx_fetched_albums {
            assert!(tx_album_ids.contains(track.id()));
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn all_by_album_empty() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_id = new_uuid("all by artist empty");

        let pool_fetched = ctx.repo.all_by_album(&ctx.pool, &fake_id).await?;
        assert!(pool_fetched.is_empty());

        let mut tx = ctx.tx().await?;
        let tx_fetched = ctx.repo.all_by_album(&mut *tx, &fake_id).await?;
        assert!(tx_fetched.is_empty());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn successfuly_delete() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_delete_result = ctx.repo.delete(&ctx.pool, saved_pool.id()).await;

        assert!(pool_delete_result.is_ok());

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_delete_resul = ctx.repo.delete(&mut *tx, saved_tx.id()).await;

        assert!(tx_delete_resul.is_ok());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn failed_to_delete() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_id = new_uuid("failed to delete");

        let pool_delete_result = ctx.repo.delete(&ctx.pool, &fake_id).await;
        assert!(pool_delete_result.is_err());

        let mut tx = ctx.tx().await?;
        let tx_delete_result = ctx.repo.delete(&mut *tx, &fake_id).await;
        assert!(tx_delete_result.is_err());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn delete_all_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let pool_chunk = &ctx.entities[0..10];
        let tx_chunk = &ctx.entities[10..20];

        let pool_saved_ids = ctx.repo.save_all(&ctx.pool, &pool_chunk).await?;
        let pool_rows_affected = ctx.repo.delete_all(&ctx.pool, &pool_saved_ids).await?;

        assert_eq!(pool_rows_affected, pool_saved_ids.len() as u64);

        let mut tx = ctx.tx().await?;
        let tx_saved_ids = ctx.repo.save_all(&mut *tx, &tx_chunk).await?;
        let tx_rows_affected = ctx.repo.delete_all(&mut *tx, &tx_saved_ids).await?;

        assert_eq!(tx_rows_affected, tx_saved_ids.len() as u64);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn failure_to_delete_all() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;

        let fake_ids = (0..10).map(|i| new_uuid(&format!("new fake id {}", i))).collect::<Vec<_>>();
        let pool_rows_affected = ctx.repo.delete_all(&ctx.pool, &fake_ids).await?;

        assert_eq!(pool_rows_affected, 0);

        let mut tx = ctx.tx().await?;
        let tx_rows_affected = ctx.repo.delete_all(&mut *tx, &fake_ids).await?;

        assert_eq!(tx_rows_affected, 0);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn delete_all_empty_vec() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;

        let ids: Vec<Uuid> = Vec::new();
        assert_eq!(ids.len(), 0);

        let rows_affected = ctx.repo.delete_all(&ctx.pool, &ids).await?;
        assert_eq!(rows_affected, 0);

        Ok(())
    }

    #[tokio::test]
    async fn id_exist() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let from_pool_exists = ctx.repo.id_exists(&ctx.pool, saved_pool.id()).await?;
        assert!(from_pool_exists);

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let from_tx_exists = ctx.repo.id_exists(&mut *tx, saved_tx.id()).await?;
        assert!(from_tx_exists);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn id_not_exist() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_id = new_uuid("should not exist");

        let from_pool_exists = ctx.repo.id_exists(&ctx.pool, &fake_id).await?;
        assert!(!from_pool_exists);

        let mut tx = ctx.tx().await?;
        let from_tx_exists = ctx.repo.id_exists(&mut *tx, &fake_id).await?;
        assert!(!from_tx_exists);

        Ok(())
    }

    #[tokio::test]
    async fn path_exists() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let from_pool_exists = ctx.repo.path_exists(&ctx.pool, saved_pool.file_path()).await?;
        assert!(from_pool_exists);

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let from_tx_exists = ctx.repo.path_exists(&mut *tx, saved_tx.file_path()).await?;
        assert!(from_tx_exists);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn path_not_exist() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_path = PathBuf::from("F:/fake/as/fuck");

        let from_pool_exists = ctx.repo.path_exists(&ctx.pool, &fake_path).await?;
        assert!(!from_pool_exists);

        let mut tx = ctx.tx().await?;
        let from_tx_exists = ctx.repo.path_exists(&mut *tx, &fake_path).await?;
        assert!(!from_tx_exists);

        Ok(())
    }

}