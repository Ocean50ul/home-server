use futures::{Stream, StreamExt};
use sqlx::{Executor, FromRow, QueryBuilder, Row, Sqlite, SqliteConnection};
use uuid::Uuid;

use crate::domain::{album::Album, BatchDeleteReport, BatchSaveOutcome, BatchSaveReport, ValidationError};
use super::{IntoUuid, RepositoryError};

#[derive(FromRow)]
struct DbAlbum {
    id: Vec<u8>,
    name: String,
    artist_id: Vec<u8>,
    year: Option<i64>
}

impl TryFrom<DbAlbum> for Album {
    type Error = AlbumConversionError;

    fn try_from(db_album: DbAlbum) -> Result<Self, Self::Error> {
        let year = match db_album.year {
            Some(int_year) => {
                if int_year <= 0 { return Err(AlbumConversionError::YearLessOrEqualToZero(int_year)); };
                Some(u32::try_from(int_year)?)
            },
            None =>  None
        };

        Ok(
            Self::new(
                Uuid::from_slice(&db_album.id)?,
                db_album.name,
                 Uuid::from_slice(&db_album.artist_id)?,
                 year
            )?
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AlbumConversionError {
    #[error("Uuid conversion error: {0}")]
    UuidConversionError(#[from] uuid::Error),

    #[error(transparent)]
    ValidationError(#[from] ValidationError),

    #[error("Error during conversion of {0} into u32.")]
    YearLessOrEqualToZero(i64),

    #[error("Album year value {0} out of range for u32.")]
    YearOutOfRange(#[from] std::num::TryFromIntError),
}

pub struct SqliteAlbumsRepository;

impl SqliteAlbumsRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteAlbumsRepository {
    pub async fn save<'e, E, A>(&self, executor: E, album: A) -> Result<Album, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        A: AsRef<Album> + Sync,
    {
        let db_album = sqlx::query_as::<_, DbAlbum>(
            "INSERT INTO albums(id, name, artist_id, year) 
            VALUES (?, ?, ?, ?)
            RETURNING *;"
        )
        .bind(album.as_ref().id())
        .bind(album.as_ref().name())
        .bind(album.as_ref().artist_id())
        .bind(album.as_ref().year())
        .fetch_one(executor)
        .await?;

        Ok(db_album.try_into()?)
    }

    pub async fn save_all<'e, A, E>(&self, executor: E, albums: &[A]) -> Result<Vec<Uuid>, RepositoryError> 
    where
        A: AsRef<Album> + Sync,
        E: Executor<'e, Database = Sqlite>
    {

        if albums.is_empty() {
            return Ok(Vec::new());
        }

        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO albums(id, name, artist_id, year) "
        );

        qbuilder.push_values(albums.iter(), |mut b, album| {
            b.push_bind(album.as_ref().id())
                .push_bind(album.as_ref().name())
                .push_bind(album.as_ref().artist_id())
                .push_bind(album.as_ref().year());
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(executor).await?;
        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.try_get(0)?;
                Ok(id)
            })
            .collect()
    }

    pub async fn batch_save<A>(&self, connection: &mut SqliteConnection, albums: &[A]) -> Result<BatchSaveReport, RepositoryError>
    where A: AsRef<Album> + Sync,
    {
        // This is per row INSERT, so there is n = albums.len() queries.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why.
        // For some reasons, you cant do it in SQL. What a shame -- domain language my ass.


        let mut batch_report = BatchSaveReport::new();

        for (index, album) in albums.iter().enumerate() {
            let album = album.as_ref();

            let id = album.id();
            let name = album.name();
            let artist_id = album.artist_id();
            let year = album.year();

            let saving_result = sqlx::query_scalar!(
                "INSERT INTO albums(id, name, artist_id, year)
                VALUES (?, ?, ?, ?)
                RETURNING id;",
                id,
                name,
                artist_id,
                year)
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

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Album>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
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
            .map_err(RepositoryError::AlbumDataMapping)
    }

    pub async fn by_name_fetch<'e, E, S>(&self, executor: E, name: S) -> Result<Option<Album>, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        S: Into<String>
    {
        let name_string = name.into();
        let db_album = sqlx::query_as::<_, DbAlbum>(
            "SELECT * FROM albums WHERE name = ? LIMIT 1;"
        )
        .bind(name_string)
        .fetch_optional(executor)
        .await?;

        db_album.map(Album::try_from)
        .transpose()
        .map_err(RepositoryError::AlbumDataMapping)
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
                Ok(db_alb) => Album::try_from(db_alb).map_err(RepositoryError::AlbumDataMapping),
                Err(err) => Err(RepositoryError::from_sqlx_error(err))
            }
        })
    }

    pub async fn all_by_artist<'e, E, ID>(&self, executor: E, artist_id: ID) -> Result<Vec<Album>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
    {
        let artist_id = artist_id.into_uuid()?;
        let db_albums = sqlx::query_as::<_, DbAlbum>(
            "SELECT id, name, artist_id, year
            FROM albums
            WHERE artist_id = ?"
        ).bind(artist_id)
        .fetch_all(executor)
        .await
        .map_err(RepositoryError::from_sqlx_error)?;

        db_albums.into_iter()
            .map(|db_album| Album::try_from(db_album).map_err(RepositoryError::AlbumDataMapping))
            .collect()
            
    }
    
    pub async fn delete<'e, ID, E>(&self, executor: E, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync,
        E: Executor<'e, Database = Sqlite>
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
            Err(RepositoryError::IdNotFound(id))
        }
    }

    pub async fn batch_delete<'e, ID>(&self, connection: &mut SqliteConnection, ids: &'e [ID]) -> Result<BatchDeleteReport, RepositoryError> 
    where 
        ID: IntoUuid + Send + Sync,
    {
        // This is per row DELETE, so there is n = ids.len() queries.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why.
        // For some reasons, you cant do it in SQL. What a shame -- domain language my ass.

        let mut report = BatchDeleteReport::new();

        for id in ids {
            let uuid = id.into_uuid()?;
            let delete_result = self.delete(&mut *connection, uuid).await;
            match delete_result {
                Ok(_) => report.deleted_ids.push(uuid.clone()),
                Err(err) => report.failed.push((uuid.clone(), err))
            }

        }

        Ok(report)
    }

    pub async fn delete_all<'e, ID, E>(&self, executor: E, ids: &'e [ID]) -> Result<u64, RepositoryError>
    where 
        ID: IntoUuid + Send + Sync,
        E: Executor<'e, Database = Sqlite>
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
        ID: IntoUuid + Send + Sync
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE id = ? LIMIT 1);",
            id
        )
        .fetch_one(executor)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            something_else => {
                let err_string = format!("Unexpected value returned from EXISTS query for ID {}: {}", id.to_string(), something_else);
                Err(RepositoryError::UnknownError(err_string))
            }
        }
    }

    pub async fn name_exists<'e, E, S>(&self, executor: E, name: S) -> Result<bool, RepositoryError> 
    where 
        E: Executor<'e, Database = Sqlite>,
        S: Into<String>
    {
        let name_string = name.into();
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE name = ? LIMIT 1);",
            name_string
        )
        .fetch_one(executor)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            something_else => {
                let err_string = format!("Unexpected value returned from EXISTS query for name {}: {}", name_string, something_else);
                Err(RepositoryError::UnknownError(err_string))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Display;

    use sqlx::{SqlitePool, Transaction};

    use super::*;
    use crate::{
        repository::{test_helpers::{prepare_db, TestSetupError}, SqliteArtistsRepository}, 
        domain::artist::Artist
    };

    const UUID_BYTES: [u8; 16] = [
        0xa5, 0x33, 0x08, 0x9f,
        0x29, 0x70, 0x43, 0x9d,
        0xbb, 0x64, 0xda, 0x39,
        0x35, 0x64, 0x36, 0x72,
    ]; // a533089f-2970-439d-bb64-da3935643672

    const ALB_NAMESPACE: Uuid = Uuid::from_bytes(UUID_BYTES);

    struct TestContext {
        pool: SqlitePool,
        repo: SqliteAlbumsRepository,
        entities: Vec<Album>,
        artist: Artist
    }

    impl TestContext {
        async fn new() -> Result<Self, TestSetupError> {
            let pool = prepare_db().await?;
            
            let art_repo = SqliteArtistsRepository::new();
            let artist = Artist::new(new_uuid("Default Artist"), "Default Artist")?;
            art_repo.save(&pool, &artist).await?;

            Ok(
                Self {
                    pool,
                    repo: SqliteAlbumsRepository::new(),
                    entities: Vec::new(),
                    artist
                }
            )
        }

        async fn tx(&self) -> Result<Transaction<Sqlite>, TestSetupError> {
            self.pool.begin().await.map_err(TestSetupError::DbError)
        }

        fn with_entities(mut self, amount: u16) -> Result<Self, TestSetupError> {
            self.entities.extend(create_albums(amount));

            Ok(self)
        }

        async fn register_artist<S>(&self, id: &S) -> Result<(), TestSetupError> 
        where S: AsRef<[u8]> + ?Sized + Display
        {
            let art_repo = SqliteArtistsRepository::new();
            let artist = Artist::new(new_uuid(id), format!("Not So Default Artist {}", id))?;
            art_repo.save(&self.pool, &artist).await?;

            Ok(())
        }
    }

    fn new_uuid<S>(name: &S) -> Uuid
    where S: AsRef<[u8]> + ?Sized
    {
        Uuid::new_v5(&ALB_NAMESPACE, name.as_ref())
    }

    fn create_albums(amount: u16) -> Vec<Album> {
        (1..=amount)
            .map(|i| {
                let album_name= format!("Test Album #{}", i);
                Album::new(
                    new_uuid(&album_name),
                    album_name,
                    new_uuid("Default Artist"),
                    Some(2000 + i as u32)
                ).expect("Error during test setup: album fields validation has failed.")
            })
            .collect()
    }

    fn create_albums_with_artist(amount: u16, artist_id: Uuid) -> Vec<Album> {
        (1..=amount)
            .map(|i| {
                let album_name= format!("Test Album ##{}", i);
                Album::new(
                    new_uuid(&album_name),
                    album_name,
                    artist_id,
                    Some(2000 + i as u32)
                ).expect("Error during test setup: album fields validation has failed.")
            })
            .collect()
    }

    #[tokio::test]
    async fn save_one_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        assert_eq!(saved_pool.id(), ctx.entities[0].id());
        assert_eq!(saved_pool.artist_id(), ctx.entities[0].artist_id());

        let mut tx = ctx.tx().await?;

        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        assert_eq!(saved_tx.id(), ctx.entities[1].id());
        assert_eq!(saved_tx.artist_id(), ctx.entities[1].artist_id());

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
    async fn something_by_name_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_fetch_outcome = ctx.repo.by_name_fetch(&ctx.pool, saved_pool.name()).await?;

        assert!(pool_fetch_outcome.is_some());
        assert!(pool_fetch_outcome.unwrap().name() == ctx.entities[0].name());

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_fetch_outcome = ctx.repo.by_name_fetch(&mut *tx, saved_tx.name()).await?;

        assert!(tx_fetch_outcome.is_some());
        assert!(tx_fetch_outcome.unwrap().name() == ctx.entities[1].name());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn none_by_name_fetch() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_name = "fake as fuck".to_string();

        let pool_fetch_outcome = ctx.repo.by_name_fetch(&ctx.pool, &fake_name).await?;
        assert!(pool_fetch_outcome.is_none());

        let mut tx = ctx.tx().await?;
        let tx_fetch_outcome = ctx.repo.by_name_fetch(&mut *tx, &fake_name).await?;
        assert!(tx_fetch_outcome.is_none());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn stream_all_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(100)?;

        let saved_ids = ctx.repo.save_all(&ctx.pool, &ctx.entities).await?;

        let mut pool_stream = ctx.repo.stream_all(&ctx.pool).await;

        while let Some(album_result) = pool_stream.next().await {
            match album_result {
                Ok(album) => {
                    assert!(saved_ids.contains(&album.id()))
                },
                Err(err) => { return Err(TestSetupError::StreamError(err)) }
            }
        }

        let mut tx = ctx.tx().await?;

        {
            let mut tx_stream = ctx.repo.stream_all(&mut *tx).await;
            while let Some(album_result) = tx_stream.next().await {
                match album_result {
                    Ok(album) => {
                        assert!(saved_ids.contains(&album.id()))
                    },
                    Err(err) => { return Err(TestSetupError::StreamError(err)) }
                }
            }
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn all_by_artist_something() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(10)?;
        ctx.register_artist("New Artist").await?;

        let pool_chunk = &ctx.entities[0..10];
        let tx_chunk = create_albums_with_artist(10, new_uuid("New Artist"));

        let pool_album_ids = ctx.repo.save_all(&ctx.pool, &pool_chunk).await?;

        let pool_fetched_albums = ctx.repo.all_by_artist(&ctx.pool, ctx.artist.id()).await?;
        assert_eq!(pool_fetched_albums.len(), 10);

        for album in pool_fetched_albums {
            assert!(pool_album_ids.contains(album.id()));
        }

        let mut tx = ctx.tx().await?;
        let tx_album_ids = ctx.repo.save_all(&mut *tx, &tx_chunk).await?;

        let tx_fetched_albums = ctx.repo.all_by_artist(&mut *tx, new_uuid("New Artist")).await?;
        assert_eq!(tx_fetched_albums.len(), 10);

        for album in tx_fetched_albums {
            assert!(tx_album_ids.contains(album.id()));
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn all_by_artist_empty() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_id = new_uuid("all by artist empty");

        let pool_fetched = ctx.repo.all_by_artist(&ctx.pool, &fake_id).await?;
        assert!(pool_fetched.is_empty());

        let mut tx = ctx.tx().await?;
        let tx_fetched = ctx.repo.all_by_artist(&mut *tx, &fake_id).await?;
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
    async fn name_exists() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let from_pool_exists = ctx.repo.name_exists(&ctx.pool, saved_pool.name()).await?;
        assert!(from_pool_exists);

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let from_tx_exists = ctx.repo.name_exists(&mut *tx, saved_tx.name()).await?;
        assert!(from_tx_exists);

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn name_not_exists() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?;
        let fake_name = "fake as fuck";

        let from_pool_exists = ctx.repo.name_exists(&ctx.pool, fake_name).await?;
        assert!(!from_pool_exists);

        let mut tx = ctx.tx().await?;
        let from_tx_exists = ctx.repo.name_exists(&mut *tx, fake_name).await?;
        assert!(!from_tx_exists);

        Ok(())
    }
}