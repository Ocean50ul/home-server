// DEPRICATED
// Old implementation with async_trait; there was a lot of lifetimes issues with some of the functions, so fuck it
// i am not doing production grade b2b saas.

use async_trait::async_trait;
use anyhow::{Error, anyhow};
use futures::Stream;

use crate::models::music::{
    Album, AlbumFilters, 
    Artist, ArtistFilters, 
    BatchResult, Filter, 
    Track, TrackFilters,
    OwnedSqlStream
};

/***********************************************************************************************
*==============================================================================================*/

/***********************************************************************************************
 * Definition of Repository trait. Represents interface of any Repository.                     *
 *                                                                                             *
 * Repository::fetch_one -- fetches one row out of DBs table with given entity ID              *
 * Repository::stream_all -- return a stream of all rows of DBs table.                         *
 * Repository::exists -- checks whether a row with given ID is in a DBs table                  *
 * Repository::delete -- deletes one row with given ID from DBs table                          *
*==============================================================================================*/
#[async_trait]
pub trait Repository<T, ID> {
    async fn fetch_one(&self, id: ID) -> Result<Option<T>, Error>;
    async fn stream_all(&self) -> impl Stream<Item = Result<T, sqlx::Error>>;

    async fn exists(&self, id: ID) -> Result<bool, Error>;
    async fn delete(&self, id: ID) -> Result<Option<T>, sqlx::Error>;
    // async fn update(&self, entity: &T) -> Result<Option<T>, Error>;
}

/***********************************************************************************************
 * Defines EntityRepository: Repository<Entity, IdType> traits.                                *
 *                                                                                             *
 * AlbumsRepository: Repository<Album, String>                                                 *
 * TracksRepository: Repository<Track, String>                                                 *
 * ArtistsRepository: Repository<Artist, String>                                               *
 *                                                                                             *
 * EntityRepository::find_one -- finds one entity according to provided EntityFilters          *
 * EntityRepository::filtered_stream_all -- returns a filtered stream of entities              *
 * EntityRepository::save_one -- saves one entity                                              *
 * EntityRepository::save_all -- saves all entities                                            *
*==============================================================================================*/
#[async_trait]
pub trait AlbumsRepository: Repository<Album, &'static str> {
    async fn find_one(&self, filters: AlbumFilters) -> Result<Option<Album>, Error>;
    fn filtered_stream_all(&self, filters: AlbumFilters) -> OwnedSqlStream<'static, Album>;

    async fn save_one(&self, entity: &Album) -> Result<Album, Error>;
    async fn save_all<'a>(&'a self, albums: &'a [Album]) -> Result<BatchResult<Album>, Error>;

    // async fn count(&self, filters: AlbumFilters) -> Result<u32, Error>;
}

#[async_trait]
pub trait TracksRepository: Repository<Track, &'static str> {
    async fn find_one(&self, filters: TrackFilters) -> Result<Option<Track>, Error>;
    fn filtered_stream_all(&self, filters: TrackFilters) -> OwnedSqlStream<'static, Track>;

    async fn save_one(&self, entity: &Track) -> Result<Track, Error>;
    async fn save_all<'a>(&'a self, tracks: &'a [Track]) -> Result<BatchResult<Track>, Error>;

    // async fn count(&self, filters: TrackFilters) -> Result<u32, Error>;
}

#[async_trait]
pub trait ArtistsRepository: Repository<Artist, &'static str> {
    async fn find_one(&self, filters: ArtistFilters) -> Result<Option<Artist>, Error>;
    fn filtered_stream_all(&self, filters: ArtistFilters) -> OwnedSqlStream<'static, Artist>;

    async fn save_one(&self, entity: &Artist) -> Result<Artist, Error>;
    async fn save_all<'a>(&'a self, artists: &'a [Artist]) -> Result<BatchResult<Artist>, Error>;

    // async fn count(&self, filters: ArtistFilters) -> Result<u32, Error>;
}

/***********************************************************************************************
 * Defines SqliteEntityRepository structs.                                                     *
*==============================================================================================*/
pub struct SqliteTracksRepository {
    pool: &'static sqlx::SqlitePool
}

pub struct SqliteAlbumsRepository {
    pool: &'static sqlx::SqlitePool
}

pub struct SqliteArtistRepository {
    pool: &'static sqlx::SqlitePool
}

/***********************************************************************************************
 * Defines SqliteEntityRepository::new() impl for SqliteEntityRepository.                      *
*==============================================================================================*/
impl SqliteTracksRepository {
    pub fn new(pool: &'static sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

impl SqliteAlbumsRepository {
    pub fn new(pool: &'static sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

impl SqliteArtistRepository {
    pub fn new(pool: &'static sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

/***********************************************************************************************
 * Defines Repository<Entity, IdType> for SqliteEntityRepository                               *
*==============================================================================================*/
#[async_trait]
impl Repository<Track, &'static str> for SqliteTracksRepository {
    async fn fetch_one(&self, id: &'static str) -> Result<Option<Track>, Error> {
        let track = sqlx::query_as::<_, Track>(
            "SELECT * FROM tracks WHERE id = ? LIMIT 1;"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(track)
    }
    
    async fn stream_all(&self) -> impl Stream<Item = Result<Track, sqlx::Error>> {
        sqlx::query_as::<_, Track>(
            "SELECT * FROM tracks;"
        ).fetch(self.pool)
    }

    async fn exists(&self, id: &'static str) -> Result<bool, Error> {
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE tracks.id = ? LIMIT 1);",
            id
        )
        .fetch_one(self.pool)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => Err(anyhow!("Exists query returned {}; Have fun debugging it!", somethingelse))
        }
    }

    async fn delete(&self, id: &'static str) -> Result<Option<Track>, sqlx::Error> {
        sqlx::query_as::<_, Track>(
            "DELETE FROM tracks WHERE tracks.id = ? RETURNING *;",
        ).bind(id)
        .fetch_optional(self.pool)
        .await
    }
}

#[async_trait]
impl Repository<Album, &'static str> for SqliteAlbumsRepository {
    async fn fetch_one(&self, id: &'static str) -> Result<Option<Album>, Error> {
        let album = sqlx::query_as::<_, Album>(
            "SELECT * FROM albums WHERE id = ? LIMIT 1;"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(album)
    }
    
    async fn stream_all(&self) -> impl Stream<Item = Result<Album, sqlx::Error>> {
        sqlx::query_as::<_, Album>(
            "SELECT * FROM albums;"
        ).fetch(self.pool)
    }

    async fn exists(&self, id: &'static str) -> Result<bool, Error> {
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE albums.id = ? LIMIT 1);",
            id
        )
        .fetch_one(self.pool)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => Err(anyhow!("Exists query returned {}; Have fun debugging it!", somethingelse))
        }
    }

    async fn delete(&self, id: &'static str) -> Result<Option<Album>, sqlx::Error> {
        sqlx::query_as::<_, Album>(
            "DELETE FROM albums WHERE albums.id = ? RETURNING *;",
        ).bind(id)
        .fetch_optional(self.pool)
        .await
    }
}

#[async_trait]
impl Repository<Artist, &'static str> for SqliteArtistRepository {
    async fn fetch_one(&self, id: &'static str) -> Result<Option<Artist>, Error> {
        let artist = sqlx::query_as::<_, Artist>(
            "SELECT * FROM artists WHERE id = ? LIMIT 1;"
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(artist)
    }
    
    async fn stream_all(&self) -> impl Stream<Item = Result<Artist, sqlx::Error>> {
        sqlx::query_as::<_, Artist>(
            "SELECT * FROM artists;"
        ).fetch(self.pool)
    }

    async fn exists(&self, id: &'static str) -> Result<bool, Error> {
        let the_answer = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM artists WHERE artists.id = ? LIMIT 1);",
            id
        )
        .fetch_one(self.pool)
        .await?;

        match the_answer {
            0 => Ok(false),
            1 => Ok(true),
            somethingelse => Err(anyhow!("Exists query returned {}; Have fun debugging it!", somethingelse))
        }
    }

    async fn delete(&self, id: &'static str) -> Result<Option<Artist>, sqlx::Error> {
        sqlx::query_as::<_, Artist>(
            "DELETE FROM artists WHERE artists.id = ? RETURNING *;",
        ).bind(id)
        .fetch_optional(self.pool)
        .await
    }
}

/***********************************************************************************************
 * Defines EntityRepository: Repository<Entity, String> for SqliteEntityRepository             *
*==============================================================================================*/
#[async_trait]
impl AlbumsRepository for SqliteAlbumsRepository {
    async fn find_one(&self, filters: AlbumFilters) -> Result<Option<Album>, Error> {
        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM albums{} LIMIT 1;", where_clauses);

        let mut query = sqlx::query_as::<_, Album>(&raw_sql);
        for binding in binds {
            query = query.bind(binding);
        }

        let album = query.fetch_optional(self.pool).await?;

        Ok(album)
    }
    fn filtered_stream_all(&self, filters: AlbumFilters) -> OwnedSqlStream<'static, Album> {
        // This function returns wrapper that owns query_string since it cant be done
        // in a different way due to async_macro. The only other way around that i've found
        // is leaking sql_string or QueryBuilder. Leaking is not something i care that much
        // in this particular case, since it is a home server that i will shut down every day
        // and i wont evoke this function a lot, but still, leak is a leak. Dont leak anything.

        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM albums{};", where_clauses);
        
        OwnedSqlStream::new(raw_sql, binds)
    }

    async fn save_one(&self, entity: &Album) -> Result<Album, Error> {
        Ok(sqlx::query_as::<_, Album>(
            "INSERT INTO albums(id, name, artist_id, year) 
            VALUES (?, ?, ?, ?)
            RETURNING *;",
        )
        .bind(entity.id).bind(&entity.name).bind(entity.artist_id).bind(entity.year)
        .fetch_one(self.pool)
        .await?)
    }
    async fn save_all<'a>(&'a self, albums: &'a [Album]) -> Result<BatchResult<Album>, Error> {
        // This is per row insert, which is not optimized since there n = tracks.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which track failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass.

        let mut batch_result = BatchResult::new(albums.len());

        for album in albums.iter() {

            let q_result = sqlx::query(
                "INSERT INTO albums(id, name, artist_id, year)
                VALUES (?, ?, ?, ?);"
            ).bind(&album.id).bind(&album.name).bind(&album.artist_id).bind(album.year)
            .execute(self.pool)
            .await;

            match q_result {
                Ok(_) => batch_result.successful += 1,
                Err(err) => {
                    match err {
                        sqlx::Error::Database(err) => {
                            if err.is_unique_violation() {
                                batch_result.skipped += 1;
                            } else {
                                batch_result.failed.push((album, sqlx::Error::Database(err)));
                            }
                        },
                        _ => batch_result.failed.push((album, err)),
                    }
                }
            }
        }

        Ok(batch_result)

    }
}

#[async_trait]
impl TracksRepository for SqliteTracksRepository {
    async fn find_one(&self, filters: TrackFilters) -> Result<Option<Track>, Error> {
        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM tracks{} LIMIT 1;", where_clauses);

        let mut query = sqlx::query_as::<_, Track>(&raw_sql);
        for binding in binds {
            query = query.bind(binding);
        }

        let track = query.fetch_optional(self.pool).await?;

        Ok(track)
    }
    fn filtered_stream_all(&self, filters: TrackFilters) -> OwnedSqlStream<'static, Track> {
        // This function returns wrapper that owns query_string since it cant be done
        // in a different way due to async_trait macro. The only other way around that i've found
        // is leaking sql_string or QueryBuilder. Leaking is not something i care that much
        // in this particular case, since it is a home server that i will shut down every day
        // and i wont evoke this function a lot, but still, leak is a leak. Dont leak anything.

        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM tracks{};", where_clauses);
        
        OwnedSqlStream::new(raw_sql, binds)
    }

    async fn save_one(&self, entity: &Track) -> Result<Track, Error> {
        let uploaded_str: &str = entity.uploaded.into();
        let saved = sqlx::query_as::<_, Track>(
            "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING *;")
            .bind(&entity.id)
            .bind(&entity.name)
            .bind(&entity.album_id)
            .bind(entity.duration)
            .bind(&entity.file_path)
            .bind(entity.file_size as i64)
            .bind(&entity.file_type)
            .bind(uploaded_str)
            .bind(entity.date_added)
            .fetch_one(self.pool)
            .await?;
        Ok(saved)
    }
    async fn save_all<'a>(&'a self, tracks: &'a [Track]) -> Result<BatchResult<Track>, Error> {
        // This is per row insert, which is not optimized since there n = tracks.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which track failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass.

        let mut batch_result = BatchResult::new(tracks.len());

        for track in tracks.iter() {

            let uploaded_str: &str = track.uploaded.into();

            let q_result = sqlx::query(
                "INSERT INTO tracks(id, name, album_id, duration, file_path, file_size, file_type, uploaded, date_added) 
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            ).bind(&track.id).bind(&track.name).bind(&track.album_id).bind(track.duration)
            .bind(&track.file_path).bind(track.file_size as i64).bind(&track.file_type).bind(uploaded_str).bind(track.date_added)
            .execute(self.pool)
            .await;

            match q_result {
                Ok(_) => batch_result.successful += 1,
                Err(err) => {
                    match err {
                        sqlx::Error::Database(err) => {
                            if err.is_unique_violation() {
                                batch_result.skipped += 1;
                            } else {
                                batch_result.failed.push((track, sqlx::Error::Database(err)));
                            }
                        },
                        _ => batch_result.failed.push((track, err)),
                    }
                }
            }
        }

        Ok(batch_result)
    }
}

#[async_trait]
impl ArtistsRepository for SqliteArtistRepository {
    async fn find_one(&self, filters: ArtistFilters) -> Result<Option<Artist>, Error> {
        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM artists{} LIMIT 1;", where_clauses);

        let mut query = sqlx::query_as::<_, Artist>(&raw_sql);
        for binding in binds {
            query = query.bind(binding);
        }

        let artist = query.fetch_optional(self.pool).await?;

        Ok(artist)
    }
    fn filtered_stream_all(&self, filters: ArtistFilters) -> OwnedSqlStream<'static, Artist> {
        // This function returns wrapper that owns query_string since it cant be done
        // in a different way due to async_macro. The only other way around that i've found
        // is leaking sql_string or QueryBuilder. Leaking is not something i care that much
        // in this particular case, since it is a home server that i will shut down every day
        // and i wont evoke this function a lot, but still, leak is a leak. Dont leak anything.

        let (where_clauses, binds) = filters.prepare_where_clauses();
        let raw_sql = format!("SELECT * FROM artists{};", where_clauses);
        
        OwnedSqlStream::new(raw_sql, binds)
    }

    async fn save_one(&self, entity: &Artist) -> Result<Artist, Error> {
        let saved = sqlx::query_as::<_, Artist>(
            "INSERT INTO artists(id, name) 
            VALUES (?, ?)
            RETURNING *;")
            .bind(&entity.id)
            .bind(&entity.name)
            .fetch_one(self.pool)
            .await?;
        Ok(saved)
    }
    async fn save_all<'a>(&'a self, artists: &'a [Artist]) -> Result<BatchResult<Artist>, Error> {
        // This is per row insert, which is not optimized since there n = tracks.len() queries.
        // My use case will NOT exceed more than 20-50 items at once, thus this implementation seems to be fine.
        // Speed was sacrificed for an ability to dynamicly insert items of the batch 
        // that are valid and to report in detailes which track failed to be saved and why. There is
        // a way to insert everything in one query, but then either granuality of errors report
        // will suffer or batch insert become all-or-nothing (non dynamic). What a shame, SQL -- domain
        // language my ass.

        let mut batch_result = BatchResult::new(artists.len());

        for artist in artists.iter() {
            let q_result = sqlx::query(
                "INSERT INTO artists(id, name)
                VALUES (?, ?);"
            ).bind(&artist.id).bind(&artist.name)
            .execute(self.pool)
            .await;

            match q_result {
                Ok(_) => batch_result.successful += 1,
                Err(err) => {
                    match err {
                        sqlx::Error::Database(err) => {
                            if err.is_unique_violation() {
                                batch_result.skipped += 1;
                            } else {
                                batch_result.failed.push((artist, sqlx::Error::Database(err)));
                            }
                        },
                        _ => batch_result.failed.push((artist, err)),
                    }
                }
            }
        }

        Ok(batch_result)
    }

}