use std::{collections::{HashMap, HashSet}, path::PathBuf};

use chrono::{Local, NaiveDateTime};
use futures::TryStreamExt;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{domain::{album::Album, artist::Artist, audiofile::AudioFileDescriptor, track::Track, uploaded::Uploaded, BatchDeleteReport, BatchSaveReport}, repository::{SqliteAlbumsRepository, SqliteArtistsRepository, SqliteTracksRepository}, services::scanner::MediaScanner};
use super::SyncServiceError;

/// Manages the synchronization between a music library on disk and the
/// application's database.
///
/// This service holds a reference to a `SqlitePool` and is intended to be
/// created for the duration of a sync operation. Its main method, `synchronize`,
/// performs a full scan and updates the database to reflect the files on disk.
pub struct MusicLibSyncService<'a> {
    artists_repo: SqliteArtistsRepository,
    albums_repo: SqliteAlbumsRepository,
    tracks_repo: SqliteTracksRepository,

    pool: &'a SqlitePool,
    music_lib_path: PathBuf,
    db_cache: DatabaseCache
}

impl<'a> MusicLibSyncService<'a> {
    /// Creates a new instance of the `MusicLibSyncService`.
    ///
    /// This constructor performs the initial, potentially expensive, work of caching
    /// the entire database state into memory for efficient processing.
    ///
    /// # Arguments
    ///
    /// * `pool` - A reference to the active `sqlx` connection pool.
    /// * `music_lib_path` - The root path of the music library to be synchronized.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be accessed or if there is an
    /// issue during the initial caching.
    pub async fn new(pool: &'a SqlitePool, music_lib_path: PathBuf) -> Result<Self, SyncServiceError> {
        let artists_repo = SqliteArtistsRepository::new();
        let albums_repo = SqliteAlbumsRepository::new();
        let tracks_repo = SqliteTracksRepository::new();

        let db_cache = MusicLibSyncService::cache_db(pool, &artists_repo, &albums_repo, &tracks_repo).await?;

        Ok(
            Self {
                artists_repo,
                albums_repo,
                tracks_repo,
                pool,
                music_lib_path,
                db_cache
            }
        )
    }

    /// Performs a full, atomic synchronization of the music library.
    ///
    /// This method executes the complete synchronization workflow:
    /// 1. Scans the filesystem for all supported audio files.
    /// 2. Compares the file list against the cached database state.
    /// 3. Computes a set of additions (new files) and deletions (missing files).
    /// 4. Applies all database changes within a single transaction.
    ///
    /// On success, it returns a `SyncServiceReport` detailing all the changes made.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem cannot be scanned or if the database
    /// transaction fails. The database will be rolled back to its original state
    /// in case of a transaction error.
    pub async fn synchronize(&self) -> Result<SyncServiceReport, SyncServiceError> {
        // Scan the filesystem to get the current, actual state of the music library.
        let scanner = MediaScanner::new(&self.music_lib_path);
        let scan_result = scanner.scan_music_lib()?;

        // Calculate the difference between the filesystem and our cached database state.
        let (additions, deletions) = self.difference(&scan_result.descriptors).await?;

        let mut tx = self.pool.begin().await?;
        let mut report = SyncServiceReport::new(Local::now().naive_local());
        
        // Apply deletions first.
        if !deletions.is_empty() {
            report.deleted_tracks = self.tracks_repo.batch_delete(&mut *tx, &deletions.track_ids).await?;
            report.deleted_albums = self.albums_repo.batch_delete(&mut *tx, &deletions.album_ids).await?;
            report.deleted_artists = self.artists_repo.batch_delete(&mut *tx, &deletions.artist_ids).await?;
        }

        // Then apply additions.
        if !additions.is_empty() {
            report.added_artists = self.artists_repo.batch_save(&mut *tx, &additions.artists.values().collect::<Vec<&Artist>>()).await?;
            report.added_albums = self.albums_repo.batch_save(&mut *tx, &additions.albums.values().collect::<Vec<&Album>>()).await?;
            report.added_tracks = self.tracks_repo.batch_save(&mut *tx, &additions.tracks.iter().collect::<Vec<&Track>>()).await?;
        }

        tx.commit().await?;
        
        Ok(report)
    }

    async fn cache_db(pool: &'a SqlitePool, artists_repo: &SqliteArtistsRepository, albums_repo: &SqliteAlbumsRepository, tracks_repo: &SqliteTracksRepository) -> Result<DatabaseCache, SyncServiceError> {

        // Fetching all the data from a DB. Memory intensive and obviously wont fit really large DBs.
        let tracks: HashMap<PathBuf, Track> = tracks_repo.stream_all(pool).await.try_collect::<Vec<_>>().await?
            .into_iter()
            .map(|t| (t.file_path().to_owned(), t))
            .collect();
        
        let artists = artists_repo.stream_all(pool).await.try_collect::<Vec<_>>().await?
            .into_iter()
            .map(|a| (a.name().to_owned(), a))
            .collect();
            
        let albums: HashMap<(String, Uuid), Album> = albums_repo.stream_all(pool).await.try_collect::<Vec<_>>().await?
            .into_iter()
            .map(|a| ((a.name().to_owned(), *a.artist_id()), a))
            .collect();

        // Creating fast lookup tables:
        let mut album_to_track_ids: HashMap<Uuid, Vec<Uuid>> = HashMap::new();      // ablum_id -> Vec<track_id>
        let mut artist_to_album_ids: HashMap<Uuid, Vec<Uuid>> = HashMap::new();     // artist_id -> Vec<album_id> of Albums that has given artist_id

        for track in tracks.values() {
            album_to_track_ids
                .entry(*track.album_id())
                .or_default()
                .push(*track.id())
        }

        for album in albums.values() {
            // Index albums by their artist for artist-level lookups.
            artist_to_album_ids
                .entry(*album.artist_id())
                .or_default()
                .push(*album.id());
        }
        
        Ok(DatabaseCache { tracks, albums, artists, album_to_track_ids, artist_to_album_ids })
    }

    fn resolve_artist_id(&self, new_files: &mut PendingAdditions, artist_name: &str) -> Result<Uuid, SyncServiceError> {
        let id = if let Some(artist) = self.db_cache.artists.get(artist_name) {
            *artist.id()
        } else if let Some(artist) = new_files.find_artist(artist_name) {
            *artist.id()
        } else {
            let new_id = Uuid::new_v4();
            let new_artist = Artist::new(new_id, artist_name)?;
            new_files.add_artist(new_artist);

            new_id
        };

        Ok(id)
    }

    fn resolve_album_id(&self, new_files: &mut PendingAdditions, alb_name: &str, art_id: Uuid, alb_year: Option<u32>) -> Result<Uuid, SyncServiceError> {
        let id = if let Some(album) = self.db_cache.albums.get(&(alb_name.to_string(), art_id)) {
            *album.id()
        } else if let Some(album) = new_files.find_album(alb_name, art_id) {
            *album.id()
        } else {
            let new_id = Uuid::new_v4();
            let new_album = Album::new(new_id, alb_name.to_string(), art_id, alb_year)?;
            new_files.add_album(new_album);

            new_id
        };

        Ok(id)
    }

    async fn find_new_files(&self, music_lib_files: &Vec<AudioFileDescriptor>) -> Result<PendingAdditions, SyncServiceError> {
        let mut new_files = PendingAdditions::new();

        for file in music_lib_files {
            if self.db_cache.tracks.contains_key(&file.path) {
                continue;
            }

            let art_id = self.resolve_artist_id(&mut new_files, &file.metadata.artist_name)?;
            let alb_id = self.resolve_album_id(&mut new_files, &file.metadata.album_name, art_id, file.metadata.album_year)?;
            let default_uploaded = Uploaded::Denis;
            let default_date = Some(Local::now().naive_local());

            let new_track = Track::new(Uuid::new_v4(), file.metadata.track_name.to_owned(), alb_id, file.metadata.track_duration, file.path.clone(), file.file_size, file.file_type.clone(), default_uploaded, default_date)?;
            new_files.add_track(new_track);

        }

        Ok(new_files)
    }

    async fn find_orphaned_entities(&self, music_lib_files: &Vec<AudioFileDescriptor>) -> Result<PendingDeletions, SyncServiceError> {

        fn is_subset<T: Eq + std::hash::Hash>(subset: &[T], superset: &HashSet<&T>) -> bool {
            subset.iter().all(|item| superset.contains(item))
        }
    
        let mut deletions = PendingDeletions::new();
        let music_lib_paths: HashSet<PathBuf> = music_lib_files.iter().map(|fd| fd.path.clone()).collect();
        
        // 1. Find all tracks whose files are missing.
        for db_track in self.db_cache.tracks.values() {
            if !music_lib_paths.contains(db_track.file_path()) {
                deletions.track_ids.push(*db_track.id());
            }
        }
        
        let tracks_to_be_deleted = deletions.track_ids.iter().collect::<HashSet<_>>();
    
        // 2. Find all orphaned albums.
        for (album_id, track_ids) in &self.db_cache.album_to_track_ids {
            if track_ids.is_empty() || is_subset(track_ids, &tracks_to_be_deleted) {
                deletions.album_ids.push(*album_id);
            }
        }
    
        let albums_to_be_deleted = deletions.album_ids.iter().collect::<HashSet<_>>();
    
        // 3. Find all orphaned artists.
        for (artist_id, album_ids) in &self.db_cache.artist_to_album_ids {
            if album_ids.is_empty() || is_subset(album_ids, &albums_to_be_deleted) {
                deletions.artist_ids.push(*artist_id);
            }
        }
    
        Ok(deletions)
    }

    async fn difference(&self, music_lib_files: &Vec<AudioFileDescriptor>) -> Result<(PendingAdditions, PendingDeletions), SyncServiceError> {
        let additions = self.find_new_files(music_lib_files).await?;
        let deletions = self.find_orphaned_entities(music_lib_files).await?;

        Ok((additions, deletions))
    }
}

#[derive(Debug)]
pub struct SyncServiceReport {
    pub deleted_tracks: BatchDeleteReport,
    pub deleted_albums: BatchDeleteReport,
    pub deleted_artists: BatchDeleteReport,

    pub added_tracks: BatchSaveReport,
    pub added_albums: BatchSaveReport,
    pub added_artists: BatchSaveReport,

    pub timestamp: NaiveDateTime,
}

impl SyncServiceReport {
    pub fn new(timestamp: NaiveDateTime) -> Self {
        Self {
            deleted_tracks: BatchDeleteReport::new(),
            deleted_albums: BatchDeleteReport::new(),
            deleted_artists: BatchDeleteReport::new(),
            
            added_tracks: BatchSaveReport::new(),
            added_albums: BatchSaveReport::new(),
            added_artists: BatchSaveReport::new(),

            timestamp
        }
    }
}

#[derive(Debug)]
struct PendingAdditions {
    artists: HashMap<String, Artist>,           // (artist_name) -> Artist
    albums: HashMap<(String, Uuid), Album>,     // (album_name, artist_id) -> Album
    tracks: HashSet<Track>
}

impl PendingAdditions {

    fn new() -> Self {
        Self {
            artists: HashMap::new(),
            albums: HashMap::new(),
            tracks: HashSet::new()
        }
    }

    fn is_empty(&self) -> bool {
        self.artists.is_empty() && self.albums.is_empty() && self.tracks.is_empty()
    }

    fn add_track(&mut self, track: Track) -> () {
        if !self.tracks.contains(&track) {
            self.tracks.insert(track);
        }
    }

    fn add_album(&mut self, album: Album) -> () {
        self.albums.entry((album.name().to_string(), *album.artist_id())).or_insert(album);
    }

    fn add_artist(&mut self, artist: Artist) -> () {
        self.artists.entry(artist.name().to_string()).or_insert(artist);
    }

    fn find_album(&self, album_name: &str, artist_id: Uuid) -> Option<&Album> {
        self.albums.get(&(album_name.to_string(), artist_id))
    }

    fn find_artist(&self, artist_name: &str) -> Option<&Artist> {
        self.artists.get(&artist_name.to_string())
    }
}

struct PendingDeletions {
    track_ids: Vec<Uuid>,
    album_ids: Vec<Uuid>,
    artist_ids: Vec<Uuid>
}

impl PendingDeletions {
    fn new() -> Self {
        Self {
            track_ids: Vec::new(),
            album_ids: Vec::new(),
            artist_ids: Vec::new()
        }
    }

    fn is_empty(&self) -> bool {
        self.track_ids.is_empty() && self.album_ids.is_empty() && self.artist_ids.is_empty()
    }
}

struct DatabaseCache {
    tracks: HashMap<PathBuf, Track>,                // PathBuf -> Track
    albums: HashMap<(String, Uuid), Album>,         // (album_name, artist_id) -> Album
    artists: HashMap<String, Artist>,               // artist_name -> Artist

    // lookup tables
    album_to_track_ids: HashMap<Uuid, Vec<Uuid>>,   // ablum_id -> Vec<track_id> of Tracks that has given album_id
    artist_to_album_ids: HashMap<Uuid, Vec<Uuid>>,  // artist_id -> Vec<album_id> of Albums that has given artist_id
}

#[cfg(test)]
pub mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;
    use walkdir::WalkDir;

    use super::*;
    use crate::{domain::audiofile::AudioFileType, services::test_helpers::*, utils::normalizations::normalize_path};

    struct TestContext {
        pool: SqlitePool,
        trk_repo: SqliteTracksRepository,
        alb_repo: SqliteAlbumsRepository,
        art_repo: SqliteArtistsRepository,
        temp_dir: TempDir,
        fixtures: Vec<PathBuf>
    }

    impl TestContext {
        async fn new() -> Result<Self, TestSetupError> {
            Ok(
                Self {
                    pool: prepare_db().await.expect("msg"),
                    trk_repo: SqliteTracksRepository::new(),
                    alb_repo: SqliteAlbumsRepository::new(),
                    art_repo: SqliteArtistsRepository::new(),
                    temp_dir: tempfile::tempdir()?,
                    fixtures: Vec::new()
                }
            )
        }

        fn with_fixtures(mut self, fixture_file_names: &[FixtureFileNames]) -> Result<Self, TestSetupError> {
            let fixture_file_names = fixture_file_names.into_iter().map(|ffm| ffm.as_str()).collect::<Vec<_>>();
            let mut selected = Vec::new();

            for entry in WalkDir::new(TEST_TRACKS_PATH).min_depth(1) {
                let entry = entry.map_err(TestSetupError::FixtureWalkerError)?;
                let name = entry.file_name()
                    .to_str()
                    .ok_or(TestSetupError::InvalidFixtureName(entry.path().to_path_buf()))?;

                if fixture_file_names.contains(&name) {
                    selected.push(entry.into_path());
                }
            }

            let mut new_paths = Vec::new();

            for src in selected {
                // safe unwrap since it was already unwrapped in the loop above
                let dest = self.temp_dir.path().join(src.file_name().unwrap());
                fs::copy(&src, &dest)?;
                new_paths.push(normalize_path(&dest));
            }

            self.fixtures = new_paths;
            Ok(self)
        }
    }

    #[tokio::test]
    async fn test_sync_service_no_op() -> Result<(), TestSetupError> {
        init_logger()?;

        // Creating ctx with tempdir that has one audiofiles in it
        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::ChevelleClosure])?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Create an Album and add it to the DB.
        let wonder_whats_next = Album::new(Uuid::new_v4(), closure_metadata.album_name, *chevelle.id(), closure_metadata.album_year)?;
        ctx.alb_repo.save(&ctx.pool, &wonder_whats_next).await?;

        // Create a track and add it to the DB.
        let trk1 = Track::new(
            Uuid::new_v4(),
            closure_metadata.track_name,
            *wonder_whats_next.id(),
            closure_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleClosure.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())
        )?;

        ctx.trk_repo.save(&ctx.pool, &trk1).await?;

        // The state is: one artist, one album and one track.
        // Expected behavior - sync service does nothing.

        // Create sync service and run it
        let sync_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = sync_service.synchronize().await?;

        // Assert that report has nothing.
        assert_eq!(report.deleted_albums.deleted_ids.len(), 0);
        assert_eq!(report.deleted_artists.deleted_ids.len(), 0);
        assert_eq!(report.deleted_tracks.deleted_ids.len(), 0);

        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 0);
        assert_eq!(report.added_tracks.successful_ids().len(), 0);

        // Assert that DB state didnt changed.
        let tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let albums = ctx.alb_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let artists = ctx.art_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        assert_eq!(tracks.len(), 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(artists.len(), 1);

        assert_eq!(tracks[0].id(), trk1.id());
        assert_eq!(albums[0].id(), wonder_whats_next.id());
        assert_eq!(artists[0].id(), chevelle.id());

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_service_add_brand_new_track() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::FlacValidMetadata])?;
        let synch_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = synch_service.synchronize().await?;

        // Assert nothing was deleted. Since DB was empty, nothing else could be done besides checking the report.
        assert_eq!(report.deleted_albums.deleted_ids.len(), 0);
        assert_eq!(report.deleted_artists.deleted_ids.len(), 0);
        assert_eq!(report.deleted_tracks.deleted_ids.len(), 0);

        // 1. Assert that REPORT has all the things it should.
        assert_eq!(report.added_tracks.successful_ids().len(), 1);
        assert_eq!(report.added_albums.successful_ids().len(), 1);
        assert_eq!(report.added_artists.successful_ids().len(), 1);

        // Create a HashSet with all the ids that report has.
        let unique_ids_from_report = vec![
            report.added_tracks.successful_ids(), 
            report.added_albums.successful_ids(), 
            report.added_artists.successful_ids()
        ]
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();

        // 2. Assert that all the things are actually inside a DB.
        let tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let albums = ctx.alb_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let artists = ctx.art_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        assert_eq!(tracks.len(), 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(artists.len(), 1);

        // 3. Assert that report and DB state are not contradicting each other.
        assert_eq!(unique_ids_from_report.len(), 3);
        assert!(unique_ids_from_report.contains(artists[0].id()));
        assert!(unique_ids_from_report.contains(albums[0].id()));
        assert!(unique_ids_from_report.contains(tracks[0].id()));

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_service_add_tracks_to_new_album_for_existing_artist() -> Result<(), TestSetupError> {
        init_logger()?;

        // Creating ctx with tempdir that has two audiofiles in it
        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::ChevelleClosure, FixtureFileNames::ChevelleForfeit])?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();
        let forfeit_metadata = FixtureFileNames::ChevelleForfeit.get_metadata();

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Adding new album and two tracks, and generating a report.
        let synch_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = synch_service.synchronize().await?;

        // Asserting that report has new album and new tracks added.
        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 1);
        assert_eq!(report.added_tracks.successful_ids().len(), 2);

        // Fetching albums and tracks from DB.
        let fetched_albums = ctx.alb_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let fetched_tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        // Asserting that fetched tracks metadata is the same that fixtures ones:
        // 1. For albums.
        assert_eq!(fetched_albums[0].artist_id(), chevelle.id());
        assert_eq!(fetched_albums[0].name(), closure_metadata.album_name);

        // 2. For tracks.
        assert_eq!(fetched_tracks.len(), 2);

        let expected_track_names: HashSet<String> = [closure_metadata.track_name, forfeit_metadata.track_name].iter().map(|s| s.to_string()).collect();
        let actual_track_names: HashSet<String> = fetched_tracks.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(expected_track_names, actual_track_names);

        let expected_paths: HashSet<&Path> = ctx.fixtures.iter().map(|p| p.as_path()).collect();
        let actual_paths: HashSet<&Path> = fetched_tracks.iter().map(|t| t.file_path().as_path()).collect();
        assert_eq!(actual_paths, expected_paths);

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_service_add_tracks_to_existing_album_and_artist() -> Result<(), TestSetupError> {
        init_logger()?;

        // Creating ctx with tempdir that has two audiofiles in it
        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::ChevelleClosure, FixtureFileNames::ChevelleForfeit])?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();
        let forfeit_metadata = FixtureFileNames::ChevelleForfeit.get_metadata();

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Create New Album and add it to the DB.
        let wonder_whats_next = Album::new(Uuid::new_v4(), closure_metadata.album_name, *chevelle.id(), closure_metadata.album_year)?;
        ctx.alb_repo.save(&ctx.pool, &wonder_whats_next).await?;

        // Adding two new tracks to existing album with existing artist.
        let synch_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = synch_service.synchronize().await?;

        // Asserting that the report is valid.
        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 0);
        assert_eq!(report.added_tracks.successful_ids().len(), 2);

        // Fetching the tracks from a DB.
        let fetched_tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        // Asserting that fetched tracks has the same metadata as fixture ones.
        let expected_track_names: HashSet<String> = [closure_metadata.track_name, forfeit_metadata.track_name].iter().map(|s| s.to_string()).collect();
        let actual_track_names: HashSet<String> = fetched_tracks.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(expected_track_names, actual_track_names);

        let expected_paths: HashSet<&Path> = ctx.fixtures.iter().map(|p| p.as_path()).collect();
        let actual_paths: HashSet<&Path> = fetched_tracks.iter().map(|t| t.file_path().as_path()).collect();
        assert_eq!(actual_paths, expected_paths);

        for track in fetched_tracks {
            assert_eq!(track.album_id(), wonder_whats_next.id());
        }

        Ok(())

    }

    #[tokio::test]
    async fn test_sync_service_delete_one_of_many_tracks_no_cascade() -> Result<(), TestSetupError> {
        init_logger()?;

        // Creating ctx with tempdir that has one audiofiles in it
        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::ChevelleClosure])?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();
        let forfeit_metadata = FixtureFileNames::ChevelleForfeit.get_metadata(); // <- this track has no audifile associated with it

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Create New Album and add it to the DB.
        let wonder_whats_next = Album::new(Uuid::new_v4(), closure_metadata.album_name, *chevelle.id(), closure_metadata.album_year)?;
        ctx.alb_repo.save(&ctx.pool, &wonder_whats_next).await?;

        // Create TWO tracks and add them to the DB.
        let trk1 = Track::new(
            Uuid::new_v4(),
            closure_metadata.track_name,
            *wonder_whats_next.id(),
            closure_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleClosure.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())
        )?;

        let trk2 = Track::new(
            Uuid::new_v4(),
            forfeit_metadata.track_name,
            *wonder_whats_next.id(),
            forfeit_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleForfeit.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())

        )?;

        ctx.trk_repo.save_all(&ctx.pool, &[&trk1, &trk2]).await?;

        // Now the state is:
        // - flac_valid_metadata.flac has associated row inside the DB and the file is inside tempdir
        // - flac_valid_metadata2.flac has associated row inside the DB, but the file is missing.

        // Create sync service and run it
        let sync_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = sync_service.synchronize().await?;

        // Assert that report has only one thing in it: deleted one track entry from a DB
        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 0);
        assert_eq!(report.added_tracks.successful_ids().len(), 0);

        assert_eq!(report.deleted_artists.deleted_ids.len(), 0);
        assert_eq!(report.deleted_albums.deleted_ids.len(), 0);
        assert_eq!(report.deleted_tracks.deleted_ids.len(), 1);

        // Asserting that the correct track was deleted.
        assert!(report.deleted_tracks.deleted_ids.contains(trk2.id()));

        // Assert that DB is in a correct state: one artist, one album, one track - trk1;
        let tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let albums = ctx.alb_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let artists = ctx.art_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        assert_eq!(tracks.len(), 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(artists.len(), 1);

        assert_eq!(tracks[0].id(), trk1.id());

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_service_delete_last_track_of_album_cascades_to_album() -> Result<(), TestSetupError> {
        init_logger()?;

        // Creating ctx with tempdir that has one audiofiles in it
        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::ChevelleClosure])?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();
        let forfeit_metadata = FixtureFileNames::ChevelleForfeit.get_metadata();

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Create two Albums and add them to the DB.
        let wonder_whats_next = Album::new(Uuid::new_v4(), closure_metadata.album_name, *chevelle.id(), closure_metadata.album_year)?;
        let should_be_deleted = Album::new(Uuid::new_v4(), "Please delete me", *chevelle.id(), closure_metadata.album_year)?;

        ctx.alb_repo.save_all(&ctx.pool, &[&wonder_whats_next, &should_be_deleted]).await?;

        // Create two tracks, associate them to two DIFFERENT albums, and add them to the DB.
        let trk1 = Track::new(
            Uuid::new_v4(),
            closure_metadata.track_name,
            *wonder_whats_next.id(),
            closure_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleClosure.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())
        )?;

        let trk2 = Track::new(
            Uuid::new_v4(),
            forfeit_metadata.track_name,
            *should_be_deleted.id(),
            forfeit_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleForfeit.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())

        )?;

        ctx.trk_repo.save_all(&ctx.pool, &[&trk1, &trk2]).await?;

        // Now the state should be: one artist, two albums, one of which has a track without associated audiofile.
        // Expected behavior: delete row from a DB without corresponding audiofile and then cascade deletion of the album 'should_be_deleted'

        // Create sync service and run it
        let sync_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = sync_service.synchronize().await?;

        // Assert that report has exactly two things deleted: one track and one album.
        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 0);
        assert_eq!(report.added_tracks.successful_ids().len(), 0);

        assert_eq!(report.deleted_artists.deleted_ids.len(), 0);
        assert_eq!(report.deleted_albums.deleted_ids.len(), 1);
        assert_eq!(report.deleted_tracks.deleted_ids.len(), 1);

        // Assert that deleted track and album was the right ones.
        assert!(report.deleted_albums.deleted_ids.contains(should_be_deleted.id()));
        assert!(report.deleted_tracks.deleted_ids.contains(trk2.id()));

        // Assert that DB is in a correct state: one artist, one album, one track.
        let tracks = ctx.trk_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let albums = ctx.alb_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;
        let artists = ctx.art_repo.stream_all(&ctx.pool).await.try_collect::<Vec<_>>().await?;

        assert_eq!(tracks.len(), 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(artists.len(), 1);

        assert_eq!(albums[0].id(), wonder_whats_next.id());
        assert_eq!(tracks[0].id(), trk1.id());

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_service_delete_last_album_of_artist_cascades_to_artist() -> Result<(), TestSetupError> {
        init_logger()?;

        // Create ctx with empty tempdir.
        let ctx = TestContext::new().await?;
        let closure_metadata = FixtureFileNames::ChevelleClosure.get_metadata();

        // Create New Artist and add it to the DB.
        let chevelle = Artist::new(Uuid::new_v4(), closure_metadata.artist_name)?;
        ctx.art_repo.save(&ctx.pool, &chevelle).await?;

        // Create New Album and add it to the DB.
        let wonder_whats_next = Album::new(Uuid::new_v4(), closure_metadata.album_name, *chevelle.id(), closure_metadata.album_year)?;
        ctx.alb_repo.save(&ctx.pool, &wonder_whats_next).await?;

        // Create the track and add to the DB.
        let trk1 = Track::new(
            Uuid::new_v4(),
            closure_metadata.track_name,
            *wonder_whats_next.id(),
            closure_metadata.track_duration,
            ctx.temp_dir.path().join(FixtureFileNames::ChevelleClosure.as_str()),
            420,
            AudioFileType::Flac,
            Uploaded::Denis,
            Some(Local::now().naive_local())
        )?;

        ctx.trk_repo.save(&ctx.pool, &trk1).await?;

        // Now the DB state should be: one track without associated audiofile, one album and one artist.
        // Expected behavior: delete orphaned album and cascade delete artist.

        // Create sync service and run it
        let sync_service = MusicLibSyncService::new(&ctx.pool, ctx.temp_dir.path().to_path_buf()).await?;
        let report = sync_service.synchronize().await?;

        // Assert that report has exactly two things deleted: one album and one artist.
        assert_eq!(report.added_artists.successful_ids().len(), 0);
        assert_eq!(report.added_albums.successful_ids().len(), 0);
        assert_eq!(report.added_tracks.successful_ids().len(), 0);

        assert_eq!(report.deleted_artists.deleted_ids.len(), 1);
        assert_eq!(report.deleted_albums.deleted_ids.len(), 1);
        assert_eq!(report.deleted_tracks.deleted_ids.len(), 1);

        Ok(())
    }
}