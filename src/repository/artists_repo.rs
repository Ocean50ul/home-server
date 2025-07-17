use futures::{Stream, StreamExt};
use sqlx::{Executor, FromRow, QueryBuilder, Row, Sqlite, SqliteConnection};
use uuid::Uuid;

use crate::domain::{BatchDeleteReport, BatchSaveOutcome, BatchSaveReport, ValidationError, artist::Artist};
use super::{IntoUuid, RepositoryError};

#[derive(FromRow)]
struct DbArtist {
    id: Vec<u8>,
    name: String
}

impl TryFrom<DbArtist> for Artist {
    type Error = ArtistConversionError;
    fn try_from(db_artist: DbArtist) -> Result<Self, Self::Error> {
        Ok(
            Self::new(Uuid::from_slice(&db_artist.id)?, db_artist.name)?
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ArtistConversionError {
    #[error("Uuid conversion error: {0}")]
    UuidConversionError(#[from] uuid::Error),

    #[error(transparent)]
    ValidationError(#[from] ValidationError)
}

pub struct SqliteArtistsRepository;

impl SqliteArtistsRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl SqliteArtistsRepository {
    pub async fn save<'e, E, A>(&self, executor: E, artist: A) -> Result<Artist, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        A: AsRef<Artist> + Sync
    {   
        let db_artist = sqlx::query_as::<_, DbArtist>(
            "INSERT INTO artists(id, name) 
            VALUES (?, ?)
            RETURNING *;")
            .bind(artist.as_ref().id())
            .bind(artist.as_ref().name())
            .fetch_one(executor)
            .await
            .map_err(RepositoryError::from_sqlx_error)?;
        
        Ok(db_artist.try_into()?)
    }

    pub async fn save_all<'e, A, E>(&self, executor: E, artists: &[A]) -> Result<Vec<Uuid>, RepositoryError> 
    where 
        A: AsRef<Artist> + Sync,
        E: Executor<'e, Database = Sqlite>
    {
        if artists.is_empty() {
            return Ok(Vec::new());
        }

        let mut qbuilder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO artists(id, name) "
        );

        qbuilder.push_values(artists.iter(), |mut builder, artist| {
            builder
            .push_bind(artist.as_ref().id())
            .push_bind(artist.as_ref().name());
        });

        qbuilder.push("RETURNING id;");

        let rows = qbuilder.build().fetch_all(executor).await
            .map_err(RepositoryError::from_sqlx_error)?;

        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.try_get(0)?;
                Ok(id)
            })
            .collect()
    }

    pub async fn batch_save<A>(&self, connection: &mut SqliteConnection, artists: &[A]) -> Result<BatchSaveReport, RepositoryError>
    where 
        A: AsRef<Artist> + Sync
    {
        // This is per row INSERT, so there is n = artists.len() queries.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which artist failed to be saved and why.
        // For some reasons, you cant do it in SQL. What a shame -- domain language my ass.

        let mut batch_report = BatchSaveReport::new();

        for (index, artist) in artists.into_iter().enumerate() {
            let artist: &Artist = artist.as_ref();

            let id = artist.id();
            let name = artist.name();

            let saving_result = sqlx::query_scalar!(
                "INSERT INTO artists(id, name)
                VALUES (?, ?)
                RETURNING id;",
                id,
                name)
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

    pub async fn by_id_fetch<'e, E, ID>(&self, executor: E, id: ID) -> Result<Option<Artist>, RepositoryError>
    where 
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
    {
        let id = id.into_uuid()?;
        let db_artist = sqlx::query_as::<_, DbArtist>(
            "SELECT * FROM artists WHERE id = ? LIMIT 1;")
            .bind(id)
            .fetch_optional(executor)
            .await
            .map_err(RepositoryError::from_sqlx_error)?;

        db_artist.map(Artist::try_from)
            .transpose()
            .map_err(RepositoryError::ArtistDataMapping)
    }

    pub async fn by_name_fetch<'e, E, S>(&self, executor: E, name: S) -> Result<Option<Artist>, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        S: Into<String>
    {
        let name_string = name.into();
        let db_album = sqlx::query_as::<_, DbArtist>(
            "SELECT * FROM artists WHERE name = ? LIMIT 1;"
        )
        .bind(name_string)
        .fetch_optional(executor)
        .await?;

        db_album.map(Artist::try_from)
        .transpose()
        .map_err(RepositoryError::ArtistDataMapping)
    }
    
    pub async fn stream_all<'e, E>(&self, executor: E) -> impl Stream<Item = Result<Artist, RepositoryError>> +'e
    where E: Executor<'e, Database = Sqlite> +'e
    {
        sqlx::query_as::<_, DbArtist>(
            "SELECT * FROM artists;")
            .fetch(executor)
            .map(|db_art_res|{
                match db_art_res {
                    Ok(db_artist) => Artist::try_from(db_artist).map_err(RepositoryError::ArtistDataMapping),
                    Err(err) => Err(RepositoryError::from_sqlx_error(err))
                }
            })
    }
    
    pub async fn delete<'e, ID, E>(&self, executor: E, id: ID) -> Result<(), RepositoryError>
    where
        ID: IntoUuid + Send + Sync,
        E: Executor<'e, Database = Sqlite>
    {
        let id = id.into_uuid()?;
        let result = sqlx::query(
            "DELETE FROM artists WHERE id = ?;")
            .bind(id)
            .execute(executor)
            .await
            .map_err(RepositoryError::from_sqlx_error)?;

        if result.rows_affected() > 0 {
            Ok(())
        } else {
            Err(RepositoryError::IdNotFound(id))
        }
    }

    pub async fn batch_delete<ID>(&self, connection: &mut SqliteConnection, ids: &[ID]) -> Result<BatchDeleteReport, RepositoryError> 
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
                Ok(_) => report.deleted_ids.push(uuid),
                Err(err) => report.failed.push((uuid, err))
            }

        }

        Ok(report)
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
            "DELETE FROM artists WHERE id IN ("
        );
        let mut separated = qbuilder.separated(", ");
        for id_like in ids.iter() {
            let uuid = id_like.into_uuid()?;
            separated.push_bind(uuid);
        }
        separated.push_unseparated(");");

        let query = qbuilder.build();
        let result = query.execute(executor)
            .await
            .map_err(RepositoryError::from_sqlx_error)?;

        Ok(result.rows_affected())
    }
    
    pub async fn id_exists<'e, ID, E>(&self, executor: E, id: ID) -> Result<bool, RepositoryError>
    where
        E: Executor<'e, Database = Sqlite>,
        ID: IntoUuid + Send + Sync
    {
        let id = id.into_uuid()?;
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM artists WHERE id = ? LIMIT 1);",
            id)
            .fetch_one(executor)
            .await
            .map_err(RepositoryError::from_sqlx_error)?;

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
            "SELECT EXISTS(SELECT 1 FROM artists WHERE name = ? LIMIT 1);",
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

    use sqlx::{SqlitePool, Transaction};

    use super::*;
    use crate::repository::{test_helpers::{prepare_db, TestSetupError}};

    const UUID_BYTES: [u8; 16] = [
        0xdc, 0xbf, 0x30, 0xd5, 
        0x05, 0x1a, 0x44, 0x1b, 
        0xaa, 0x30, 0xe3, 0xa8, 
        0x23, 0xa1, 0x7c, 0xe7
    ]; // dcbf30d5-051a-441b-aa30-e3a823a17ce7

    const ART_NAMESPACE: Uuid = Uuid::from_bytes(UUID_BYTES);

    struct TestContext {
        pool: SqlitePool,
        repo: SqliteArtistsRepository,
        entities: Vec<Artist>,
    }

    impl TestContext {
        async fn new() -> Result<Self, TestSetupError> {
            Ok(
                Self {
                    pool: prepare_db().await?,
                    repo: SqliteArtistsRepository::new(),
                    entities: Vec::new()
                }
            )
        }

        async fn tx(&self) -> Result<Transaction<Sqlite>, TestSetupError> {
            self.pool.begin().await.map_err(TestSetupError::DbError)
        }

        fn with_entities(mut self, amount: u16) -> Result<Self, TestSetupError> {
            self.entities.extend(create_artists(amount));

            Ok(self)
        }
    }

    fn new_uuid<S>(name: &S) -> Uuid
    where S: AsRef<[u8]> + ?Sized
    {
        Uuid::new_v5(&ART_NAMESPACE, name.as_ref())
    }

    fn create_artists(amount: u16) -> Vec<Artist> {
        (1..=amount)
            .map(|i| {
                let artist_name= format!("Test Artist #{}", i);
                Artist::new(
                    new_uuid(&artist_name),
                    artist_name
                ).expect("Error during test setup: Artist fields validation has failed.")
            })
            .collect()
    }

    #[tokio::test]
    async fn save_one_success() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let saved_pool = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;

        assert_eq!(ctx.entities[0].id(), saved_pool.id());
        assert_eq!(ctx.entities[0].name(), saved_pool.name());

        let mut tx = ctx.tx().await?;
        let saved_tx = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;

        assert_eq!(ctx.entities[1].id(), saved_tx.id());
        assert_eq!(ctx.entities[1].name(), saved_tx.name());

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn save_one_failure() -> Result<(), TestSetupError> {
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
        let first_chunk = &ctx.entities[0..10];
        let second_chuck = &ctx.entities[10..20];

        let saved_pool_ids = ctx.repo.save_all(&ctx.pool, first_chunk).await?;

        for artist in first_chunk {
            assert!(saved_pool_ids.contains(&artist.id()));
        }

        let mut tx = ctx.tx().await?;
        let saved_tx_ids = ctx.repo.save_all(&mut *tx, second_chuck).await?;
        
        for artist in second_chuck {
            assert!(saved_tx_ids.contains(&artist.id()));
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn save_all_failure() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(20)?;
        let first_chunk = &ctx.entities[0..10];
        let second_chuck = &ctx.entities[10..20];

        ctx.repo.save_all(&ctx.pool, &first_chunk).await?;
        let duplicate_pool_save = ctx.repo.save_all(&ctx.pool, first_chunk).await;

        assert!(duplicate_pool_save.is_err());

        let mut tx = ctx.tx().await?;
        ctx.repo.save_all(&mut *tx, &second_chuck).await?;
        let duplicate_tx_save = ctx.repo.save_all(&mut *tx, second_chuck).await;

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

        let pool_saved_artist = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_fetch_outcome = ctx.repo.by_id_fetch(&ctx.pool, pool_saved_artist.id()).await?;

        assert!(pool_fetch_outcome.is_some());
        assert!(pool_fetch_outcome.unwrap().id() == ctx.entities[0].id());

        let mut tx = ctx.tx().await?;
        let tx_saved_artist = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_fetch_outcome = ctx.repo.by_id_fetch(&mut *tx, tx_saved_artist.id()).await?;

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

        while let Some(artist_result) = pool_stream.next().await {
            match artist_result {
                Ok(artist) => {
                    assert!(saved_ids.contains(&artist.id()))
                },
                Err(err) => { return Err(TestSetupError::StreamError(err)) }
            }
        }

        let mut tx = ctx.tx().await?;

        {
            let mut tx_stream = ctx.repo.stream_all(&mut *tx).await;
            while let Some(artist_result) = tx_stream.next().await {
                match artist_result {
                    Ok(artist) => {
                        assert!(saved_ids.contains(&artist.id()))
                    },
                    Err(err) => { return Err(TestSetupError::StreamError(err)) }
                }
            }
        }

        tx.commit().await?;

        Ok(())
    }

    #[tokio::test]
    async fn successfuly_delete() -> Result<(), TestSetupError> {
        let ctx = TestContext::new().await?.with_entities(2)?;

        let pool_saved_artist = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let pool_delete_result = ctx.repo.delete(&ctx.pool, pool_saved_artist.id()).await;

        assert!(pool_delete_result.is_ok());

        let mut tx = ctx.tx().await?;
        let tx_saved_artist = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let tx_delete_resul = ctx.repo.delete(&mut *tx, tx_saved_artist.id()).await;

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
        let first_chunk = &ctx.entities[0..10];
        let second_chuck = &ctx.entities[10..20];

        let pool_saved_ids = ctx.repo.save_all(&ctx.pool, &first_chunk).await?;
        let pool_rows_affected = ctx.repo.delete_all(&ctx.pool, &pool_saved_ids).await?;

        assert_eq!(pool_rows_affected, pool_saved_ids.len() as u64);

        let mut tx = ctx.tx().await?;
        let tx_saved_ids = ctx.repo.save_all(&mut *tx, &second_chuck).await?;
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

        let pool_saved_artist = ctx.repo.save(&ctx.pool, &ctx.entities[0]).await?;
        let from_pool_exists = ctx.repo.id_exists(&ctx.pool, pool_saved_artist.id()).await?;
        assert!(from_pool_exists);

        let mut tx = ctx.tx().await?;
        let tx_saved_artist = ctx.repo.save(&mut *tx, &ctx.entities[1]).await?;
        let from_tx_exists = ctx.repo.id_exists(&mut *tx, tx_saved_artist.id()).await?;
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
