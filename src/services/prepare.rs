use std::{collections::HashMap, env::{self, VarError}, fs::{create_dir, create_dir_all, read_to_string, remove_dir_all, remove_file, write, File}, io::{Read, Write}, path::{Path, PathBuf}, process::Command};
use tokio::io::AsyncWriteExt;

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use sevenz_rust2::{self, ArchiveReader, Password};

use crate::{domain::audiofile::AudioFileType, utils::config::{get_config, Config, ConfigLoadingError}};

const FFMPEG_EXECUTABLE_NAME: &str = "ffmpeg.exe";
const FFMPEG_ARCHIVE_NAME: &str = "ffmpeg_zip.7z";

#[derive(Debug, thiserror::Error)]
pub enum PrepareServiceError {

    #[error(transparent)]
    ConfigLoadingError(#[from] ConfigLoadingError),

    #[error(transparent)]
    RequestError(#[from] reqwest::Error),

    #[error("Error creating destination file for a download: {0}")]
    ErrorCreatingDestinationFile(std::io::Error),

    #[error("Error during ffmpeg copy into a destination file: {0}")]
    ErrorCopyingIntoDestinationFile(std::io::Error),

    #[error("My Big Beautiful Parsing Function has failed to parse checksums out of html string")]
    FailedToParseChecksums(),

    #[error("ffmpeg.exe seems to be still missing after downloading and extracting steps was done.")]
    FfmpegDoesntExist(),

    #[error("Checksums do not match. Expected: {expected}, Got: {actual}")]
    ChecksumMismatch { actual: String, expected: String },

    #[error("Could not open file '{path}': {source}")]
    FileOpenError { path: PathBuf, #[source] source: std::io::Error },

    #[error("Could not read from file '{path}': {source}")]
    FileReadError { path: PathBuf, #[source] source: std::io::Error },

    #[error("Could not remove file '{path}': {source}")]
    FileRemoveError { path: PathBuf, #[source] source: std::io::Error },

    #[error("Could not create file '{path}': {source}")]
    FileCreateError { path: PathBuf, #[source] source: std::io::Error },

    #[error("Could not write to a file '{path}': {source}")]
    FileWriteError { path: PathBuf, #[source] source: std::io::Error },

    #[error("Could not create dir '{path}': {source}")]
    DirCreateError { path: PathBuf, #[source] source: std::io::Error },
    
    #[error("Failed to extract ffmpeg.exe from archive: {0}")]
    ErrorExtractingFfmpeg(sevenz_rust2::Error),

    #[error(transparent)]
    FixtureSetupError(#[from] FixturesSetupError),

    #[error("Request returned with error status: ")]
    RequestFailureStatus(String),

    #[error("Failed to read the file from ffmpeg archive: {0}")]
    FailedToReadTheFileFromArchive(sevenz_rust2::Error),

    #[error("Failed to find ffmpeg.exe inside the archive! The name provided: {0}; ends_with didnt worked out!")]
    FailedToFindFFmpegInsideArchive(String),

    #[error("for_each_entries has returned with an error: {0}")]
    ForEachError(sevenz_rust2::Error)
}

/* ======================= FFMPEG PREPARATION PART ======================= */

fn ffmpeg_exists(path: &Path) -> bool {
    path.exists()
}

async fn download_ffmpeg_zip_essentials(dest_file_path: &Path, url: &str) -> Result<(), PrepareServiceError> {
    println!("Downloading ffmpeg from {}", url);
    
    let mut dest_file = tokio::fs::File::create(dest_file_path).await
        .map_err(|err| PrepareServiceError::ErrorCreatingDestinationFile(err))?;

    let client = Client::new();
    let mut response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(PrepareServiceError::RequestFailureStatus(response.status().to_string()));
    }

    let pb: ProgressBar;
    if let Some(total_size) = response.content_length() {
        // --- CASE 1: Content-Length EXISTS ---
        pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"));
    } else {
        // --- CASE 2: Content-Length IS MISSING ---
        pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {bytes} downloaded ({bytes_per_sec})")
            .unwrap());
    }

    while let Some(chunk) = response.chunk().await? {
        dest_file
            .write_all(&chunk).await
            .map_err(|err| PrepareServiceError::ErrorCopyingIntoDestinationFile(err))?;

        pb.inc(chunk.len() as u64);
    }

    dest_file.flush().await.map_err(|e| PrepareServiceError::ErrorCopyingIntoDestinationFile(e))?;
    pb.finish_with_message("Download complete");

    Ok(())
}

pub async fn get_checksums(checksum_url: &str) -> Result<String, PrepareServiceError> {
    let client = Client::new();
    let response = client.get(checksum_url).send().await?;

    Ok(response.text().await?)
}

fn verify_checksums(ffmpeg_zip_path: &Path, expected_checksum: String) -> Result<(), PrepareServiceError> {
    let mut file = File::open(ffmpeg_zip_path).map_err(|err| PrepareServiceError::FileReadError{ path: ffmpeg_zip_path.to_path_buf(), source: err})?;
    let mut hasher = Sha256::new();

    let mut buffer = vec![0; 8192];

    loop {
        let n = file.read(&mut buffer).map_err(|err| PrepareServiceError::FileReadError{ path: ffmpeg_zip_path.to_path_buf(), source: err})?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    
    let hash_bytes = hasher.finalize();
    let hash_hex = format!("{:x}", hash_bytes);

    if !(hash_hex == expected_checksum) {
        return Err(PrepareServiceError::ChecksumMismatch{actual: hash_hex, expected: expected_checksum});
    }

    Ok(())
}

pub fn unzip_ffmpeg(zip_path: &Path, file_name: &str, unzip_dest: &Path) -> Result<(), PrepareServiceError> {

    let mut archive_reader = ArchiveReader::open(zip_path, Password::empty())
        .map_err(PrepareServiceError::ErrorExtractingFfmpeg)?;

    let mut file_found = false;

    let result = archive_reader.for_each_entries(|entry, reader| {
        if entry.is_directory() {
            return Ok(true); // Continue
        }

        if !file_found && entry.name().ends_with(file_name) {
            println!("\nExtracting ffmpeg.exe fron an archive..");
            
            let total_size = entry.size();
            let pb = ProgressBar::new(total_size);
            pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{bar:40.yellow/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")
                            .unwrap()
                            .progress_chars("=> ")
            );

            let dest_path = unzip_dest.join(file_name);
            let mut dest_file = File::create(&dest_path)?;

            let mut buf = [0; 8192];
            loop {
                let bytes_read = reader.read(&mut buf)?;

                if bytes_read == 0 {
                    break;
                }

                dest_file.write_all(&buf[..bytes_read])?;
                
                pb.inc(bytes_read as u64);
            }
            
            pb.finish_with_message("Extraction complete.");
            file_found = true;
            
            return Ok(false);
        }

        Ok(true)
    });

    if let Err(e) = result {
        return Err(PrepareServiceError::ForEachError(e));
    }
    
    if !file_found {
        return Err(PrepareServiceError::FailedToFindFFmpegInsideArchive(file_name.to_string()));
    }

    Ok(())
}

pub async fn prepare_ffmpeg(config: &Config) -> Result<(), PrepareServiceError> {
    let ffmpeg_exe_path = &config.media.ffmpeg_exe_path;

    if ffmpeg_exists(&ffmpeg_exe_path) {
        return Ok(());
    }
    let zip_path =config.media.ffmpeg_dir_path.join(FFMPEG_ARCHIVE_NAME);
    let gyan_mirror = &config.media.ffmpeg_donwload_mirror;
    download_ffmpeg_zip_essentials(&zip_path, gyan_mirror).await?;

    let checksum_url = &config.media.ffmpeg_sha_download_mirror;
    let expected_checksum = get_checksums(checksum_url).await?;
    verify_checksums(&zip_path, expected_checksum)?;

    unzip_ffmpeg(&zip_path, FFMPEG_EXECUTABLE_NAME, &config.media.ffmpeg_dir_path)?;

    if !ffmpeg_exists(&ffmpeg_exe_path) {
        return Err(PrepareServiceError::FfmpegDoesntExist())
    }

    println!("\nCleaning things up..");
    remove_file(&zip_path).map_err(|err| PrepareServiceError::FileRemoveError{path: zip_path.to_path_buf(), source: err})?;

    Ok(())
}

/* ======================= END OF FFMPEG PREPARATION PART ======================= */




/* ======================= DB PREPARATION PART ======================= */
pub fn prepare_db(config: &Config) -> Result<(), PrepareServiceError> {
    let db_path = &config.database.path;

    if db_path.exists() {
        return Ok(())
    }

    File::create(&db_path).map_err(|err| PrepareServiceError::FileCreateError {path: db_path.to_path_buf(), source: err})?;

    Ok(())
}
/* ======================= END DB PREPARATION PART ======================= */




/* ======================= DIRS PREPARATION PART ======================= */
pub fn prepare_dirs(config: &Config) -> Result<(), PrepareServiceError> {

    // bad practice to unwrap things, buuuut
    let db_path = config.database.path.parent().map(|p| p.to_path_buf()).unwrap();

    let paths = vec![
        &config.media.resampled_music_path,
        &config.media.video_path,
        &config.media.video_path,
        &config.media.ffmpeg_dir_path,
        &config.media.test_fixtures_path,
        &config.media.filesharing_path,
        &db_path
    ];

    for path in paths {
        create_dir_all(path)
            .map_err(|err| PrepareServiceError::DirCreateError { path: path.to_path_buf(), source: err})?;
    }
    Ok(())
}
/* ======================= END DIRS PREPARATION PART ======================= */




/* ======================= TEST-FIXTURES PREPARATION PART ======================= */
#[derive(Debug, thiserror::Error)]
pub enum FixturesSetupError {

    #[error("Fixtures setup has failed. I/O error has occured: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Fixtures setup has failed. Unable to get SystemRoot env variable: {0}")]
    SystemRootVariableNotFound(#[from] VarError),

    #[error("Fixtures setup has failed: icacls.exe doesn't exist. GG.")]
    IcaclsNotFound(),

    #[error("Fixtures setup has failed. Unable to parse path during: {0}")]
    InvalidPath(String),

    #[error("Fixtures setup has failed. Icacls returned with an error: {0}")]
    IcaclsCommandError(String),

    #[error("Fixtures setup has failed. Error during fixtures state serialization: {0}")]
    FixturesCacheSerializationError(#[from] serde_json::Error),

    #[error("Fixtures setup has failed. Error during fixtures creation - unsopporetd type: {0}")]
    UnsupportedFileType(String),

    #[error("Fixtures setup has failed. Ffmpeg returned with an error: {0}")]
    FfmpegCommandError(String)
}

pub struct AudioFixture {
    pub path: String,
    pub metadata: HashMap<String, String>
}

impl AudioFixture {
    pub fn new(audio_type: AudioFileType, config: &Config) -> Result<AudioFixture, FixturesSetupError> {
        let (file_name, metadata) = match audio_type {
            AudioFileType::Flac => (
                "flac_valid_metadata.flac",
                HashMap::from([
                    ("title".to_string(), "FLAC test title".to_string()),
                    ("artist".to_string(), "FLAC test artist".to_string()),
                    ("album".to_string(), "FLAC test album".to_string()),
                    ("genre".to_string(), "FLAC test genre".to_string()),
                    ("date".to_string(), "2023".to_string()),
                    ("track".to_string(), "1/1".to_string()),
                    ("comment".to_string(), "FLAC test comment".to_string())
                ])
            ),

            AudioFileType::Mp3 => (
                "mp3_valid_metadata.mp3",
                HashMap::from([
                    ("title".to_string(), "MP3 test title".to_string()),
                    ("artist".to_string(), "MP3 test artist".to_string()),
                    ("album".to_string(), "MP3 test album".to_string()),
                    ("genre".to_string(), "MP3 test genre".to_string()),
                    ("date".to_string(), "2023".to_string()),
                    ("track".to_string(), "1".to_string()),
                    ("comment".to_string(), "MP3 test comment".to_string())
                ])
            ),

            AudioFileType::Wav => (
                "wav_valid_metadata.wav",
                HashMap::from([
                    ("title".to_string(), "WAV test title".to_string()),
                    ("artist".to_string(), "WAV test artist".to_string()),
                    ("album".to_string(), "WAV test album".to_string()),
                    ("genre".to_string(), "WAV test genre".to_string()),
                    ("date".to_string(), "2023".to_string()),
                    ("track".to_string(), "1".to_string()),
                    ("comment".to_string(), "WAV test comment".to_string())
                ])
            ),

            unsupported => return Err(FixturesSetupError::UnsupportedFileType(unsupported.as_str().to_string()))
        };

        let path = config.media.test_fixtures_path
            .join("files")
            .join(file_name)
            .to_string_lossy()
            .to_string();   

        Ok(
            Self {
                path,
                metadata
            }
        )
    }
}

#[derive(Serialize, Deserialize)]
pub struct FixturesContext {
    pub fixture_path: PathBuf,
    pub stripped_files: Vec<PathBuf>,
    pub stripped_dirs: Vec<PathBuf>,
    pub fixtures_cache_path: PathBuf
}

impl FixturesContext {
    pub fn new() -> Self {
        Self {
            fixture_path: PathBuf::from("./test_fixtures"),
            stripped_files: Vec::new(),
            stripped_dirs: Vec::new(),
            fixtures_cache_path: PathBuf::from("./test_fixtures/fixtures_state.json")
        }
    }

    pub fn cache(&self) -> Result<(), FixturesSetupError> {
        let json_str = serde_json::to_string(self)?;
        
        write(&self.fixtures_cache_path, json_str.as_bytes())?;
        
        Ok(())
    }

    pub fn cache_exists(&self) -> bool {
        self.fixtures_cache_path.exists()
    }
}

pub fn make_inaccessible_dir(name: &str, fctx: &mut FixturesContext) -> Result<PathBuf, FixturesSetupError> {
    let dir_path = fctx.fixture_path.join(name);

    create_dir(&dir_path)?;
    strip_permissions(&dir_path)?;

    // Track for cleanup
    fctx.stripped_dirs.push(dir_path.clone());

    Ok(dir_path)
}

pub fn make_inaccessable_file(path: &Path, fctx: &mut FixturesContext) -> Result<(), FixturesSetupError> {
    write(path, b"test")?;
    strip_permissions(&path)?;
    
    // Track for cleanup
    fctx.stripped_files.push(path.to_path_buf());

    Ok(())
}

fn get_icacls_path() -> Result<PathBuf, FixturesSetupError> {
    let system_root = env::var("SystemRoot").map_err(|e| FixturesSetupError::SystemRootVariableNotFound(e))?;
    let icacls_path = Path::new(&system_root).join("system32").join("icacls.exe");

    if !icacls_path.exists() {
        return Err(FixturesSetupError::IcaclsNotFound());
    }

    Ok(icacls_path)
}

fn strip_permissions(path: &Path) -> Result<(), FixturesSetupError> {
    let icacls_path = get_icacls_path()?;

    let output = Command::new(&icacls_path)
        .args(&[
            path.to_str().ok_or_else(|| FixturesSetupError::InvalidPath(path.to_string_lossy().to_string()))?,
            "/inheritance:r",  // Remove inheritance
            "/deny",
            "Everyone:(F)",    // Deny full control to everyone
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FixturesSetupError::IcaclsCommandError(stderr.to_string()))
    }

    Ok(())
}

fn restore_permissions(path: &Path) -> Result<(), FixturesSetupError> {
    let icacls_path = get_icacls_path()?;

    let output = Command::new(&icacls_path)
        .args(&[
            path.to_str().ok_or_else(|| FixturesSetupError::InvalidPath(path.to_string_lossy().to_string()))?,
            "/reset",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FixturesSetupError::IcaclsCommandError(stderr.to_string()))
    }
    Ok(())
}

pub fn prepare_fixtures(fctx: &mut FixturesContext) -> Result<(), FixturesSetupError> {
    if fctx.fixtures_cache_path.exists() {
        // right now assume that if cache exist, then all the fixutres are also presented.
        return Ok(());
    }

    create_dir_all(fctx.fixture_path.join("files"))?;
    create_dir_all(fctx.fixture_path.join("dirs/accessible_dir"))?;

    make_inaccessible_dir("dirs/inaccessible_dir", fctx)?;
    make_inaccessible_dir("dirs/accessible_dir/inaccessible_dir", fctx)?;

    fctx.cache()?;

    Ok(())
}

pub fn create_fixture_audio_files(config: &Config) -> Result<(), FixturesSetupError> {
    let audio_fixtures = vec![
        AudioFixture::new(AudioFileType::Flac, config)?,
        AudioFixture::new(AudioFileType::Mp3, config)?,
        AudioFixture::new(AudioFileType::Wav, config)?
    ];

    for fix in audio_fixtures {
        let mut cmd = Command::new(&config.media.ffmpeg_exe_path);

        let mut args = vec![
            "-y".to_string(),
            "-f".to_string(), "lavfi".to_string(),
            "-i".to_string(), "sine=frequency=880:duration=5".to_string(),
        ];

        for (key, value) in fix.metadata {
            args.push("-metadata".to_string());
            args.push(format!("{}={}", key, value));
        }

        args.push(fix.path);

        let output = cmd
            .args(&args)
            .output()?;

        if !output.status.success() {

            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FixturesSetupError::FfmpegCommandError(stderr.to_string()));
        }
}

    Ok(())
}

pub fn cleanup(fixtures_state_json: &Path) -> Result<(), FixturesSetupError> {
    let json_str = read_to_string(fixtures_state_json)?;
    let mut fctx: FixturesContext = serde_json::from_str(&json_str)?;

    // Restore files first
    for file in &fctx.stripped_files {
        if let Err(err) = restore_permissions(file) {
            eprintln!("Warning: Failed to restore permissions for file {:?}: {}", file, err);
        }
    }

    // Then restore directories (deepest first)
    fctx.stripped_dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    for dir in &fctx.stripped_dirs {
        if let Err(err) = restore_permissions(dir) {
            // Log.
            eprintln!("Warning: Failed to restore permissions for {:?}: {}.\nTrying takeown..", dir, err);
            
            // Try alternative approach with takeown
            let takeown_ountcome = Command::new("takeown")
                .arg("/f")
                .arg(dir)
                .arg("/r")
                .arg("/d")
                .arg("y")
                .output();

            if let Err(err) = takeown_ountcome {
                eprintln!("Warning: Failed to restore permissions with takeown for {:?}: {}.\nTrying icacls again..", dir, err);
            }
            
            // Try icacls again
            let icacls_2 = restore_permissions(dir);

            if let Err(err) = icacls_2 {
                eprintln!("Warning: Failed to restore permissions with for {:?}: {}.\nGG DUDE, I TRIED.", dir, err);
            }
        }
    }

    remove_dir_all(fctx.fixture_path)?;

    Ok(())
}

pub async fn run_prepare_devspace() -> Result<(), PrepareServiceError> {
    let config = get_config()?;

    prepare_dirs(config)?;
    prepare_db(config)?;
    prepare_ffmpeg(config).await?;

    let mut fixtures_context = FixturesContext::new();
    prepare_fixtures(&mut fixtures_context)?;
    create_fixture_audio_files(config)?;

    Ok(())
}

pub async fn run_prepare_userspace() -> Result<(), PrepareServiceError> {
    let config = get_config()?;

    prepare_dirs(config)?;
    prepare_db(config)?;
    prepare_ffmpeg(config).await?;

    Ok(())
}

#[cfg(test)]
pub mod tests {
    use std::io::Write;

    use tempfile::TempDir;

    use crate::utils::config::{DatabaseConfig, MediaConfig, ServerConfig};

    use super::*;

    #[derive(Debug, thiserror::Error)]
    enum TestSetupError {
        #[error("I/O error: {0}")]
        IOError(#[from] std::io::Error),

        #[error("prepare_dirs returned with an error: {0}")]
        FailedToPrepareDirs(PrepareServiceError),

        #[error("prepare_db returned with an error: {0}")]
        FailedToPrepareDb(PrepareServiceError),

        #[error("FixturesContext::cache() returned with an error: {0}")]
        FailedToCacheFixtures(FixturesSetupError),

        #[error("error while deserializing fixture_state.json: {0}")]
        FailedToDeserializeFixturesState(serde_json::Error),

        #[error("error during dummy file compression: {0}")]
        FailedToCompressDummy(sevenz_rust2::Error),

        #[error("prepare_ffmpeg returned with an error: {0}")]
        FailedToPrepareFfmpeg(PrepareServiceError),

        #[error("prepare_fixtures returned with an error: {0}")]
        FaileToPrepareFixtures(FixturesSetupError),

        #[error(">>>>>WARNING<<<<< cleanup function has returned with an error: {source}; you need to clean things up manualy here: {fixtures_path}")]
        FailedToCleanThingsUp { source: FixturesSetupError, fixtures_path: PathBuf }
    }

    struct TestContext {
        tempdir: TempDir,
        config_mock: Config
    }

    impl TestContext {
        fn new() -> Result <Self, TestSetupError> {
            let tempdir = TempDir::new()?;
            Ok(
                Self {
                    config_mock: Config {
                        server: ServerConfig {
                            host: "0.0.0.0".to_string(),
                            port: 8080
                        },

                        database: DatabaseConfig {
                            path: tempdir.path().join("data/db/database.db")
                        },

                        media: MediaConfig {
                            music_path: tempdir.path().join("data/media/music"),
                            video_path: tempdir.path().join("data/media/video"),
                            filesharing_path: tempdir.path().join("data/filesharing"),
                            ffmpeg_dir_path: tempdir.path().join("ffmpeg"),
                            ffmpeg_exe_path: tempdir.path().join("ffmpeg/ffmpeg.exe"),
                            ffmpeg_donwload_mirror: "mock this!".to_string(),
                            ffmpeg_sha_download_mirror: "mock this".to_string(),
                            test_fixtures_path: tempdir.path().join("test_fixtures"),
                            resampled_music_path: tempdir.path().join("data/media/music/.resampled"),
                            audio_fixtures_json_path: tempdir.path().join("./audio_fixtures.json")
                        }
                    },

                    tempdir: tempdir
                }
            )
        }

        fn set_ffmpeg_dl_mirror(&mut self, url: String) -> () {
            self.config_mock.media.ffmpeg_donwload_mirror = url;
        }

        fn set_ffmpeg_sha_dl_mirror(&mut self, url: String) -> () {
            self.config_mock.media.ffmpeg_sha_download_mirror = url;
        }
    }

    #[tokio::test]
    async fn test_ffmpeg_exists_when_present() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;
        let ffmpeg_path  = ctx.tempdir.path().join(FFMPEG_EXECUTABLE_NAME);
        File::create(&ffmpeg_path)?;

        assert!(ffmpeg_exists(&ffmpeg_path));

        Ok(())
    }

    #[tokio::test]
    async fn test_ffmpeg_exists_when_absent() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;
        let ffmpeg_path  = ctx.tempdir.path().join(FFMPEG_EXECUTABLE_NAME);

        assert!(!ffmpeg_exists(&ffmpeg_path));

        Ok(())
    }

    #[tokio::test]
    async fn test_prepare_dirs() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;

        prepare_dirs(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDirs(err))?;
        let db_path = ctx.config_mock.database.path.parent().map(|p| p.to_path_buf()).unwrap();


        assert!(db_path.exists());
        assert!(ctx.config_mock.media.ffmpeg_dir_path.exists());
        assert!(ctx.config_mock.media.filesharing_path.exists());
        assert!(ctx.config_mock.media.music_path.exists());
        assert!(ctx.config_mock.media.resampled_music_path.exists());
        assert!(ctx.config_mock.media.video_path.exists());
        assert!(ctx.config_mock.media.test_fixtures_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_prepare_db_creates_file() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;
        prepare_dirs(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDirs(err))?;

        prepare_db(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDb(err))?;

        assert!(ctx.config_mock.database.path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_fixture_context_cache() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;
        prepare_dirs(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDirs(err))?;

        let inacc_dir1 = PathBuf::from("./test-fixtures/dirs/inaccessible_dir");
        let inacc_dir2 = PathBuf::from("./test-fixtures/dirs/accessible_dir/inaccessible_dir");
        let inacc_file = PathBuf::from("./test-fixtures/dirs/accessible_dir/inaccessible_file.flac");

        let mut fxtr_context = FixturesContext {
            fixture_path: ctx.config_mock.media.test_fixtures_path.clone(),
            stripped_dirs: Vec::new(),
            stripped_files: Vec::new(),
            fixtures_cache_path: ctx.config_mock.media.test_fixtures_path.join("fixtures_state.json")
        };

        assert!(fxtr_context.fixture_path.exists());

        fxtr_context.stripped_dirs.push(inacc_dir1.clone());
        fxtr_context.stripped_dirs.push(inacc_dir2.clone());
        fxtr_context.stripped_files.push(inacc_file.clone());

        fxtr_context.cache().map_err(|err| TestSetupError::FailedToCacheFixtures(err))?;
        assert!(fxtr_context.fixtures_cache_path.exists());

        let json_str = read_to_string(fxtr_context.fixtures_cache_path)?;
        let cached_fxtr: FixturesContext = serde_json::from_str(&json_str).map_err(|err| TestSetupError::FailedToDeserializeFixturesState(err))?;

        assert!(cached_fxtr.stripped_dirs.contains(&inacc_dir1));
        assert!(cached_fxtr.stripped_dirs.contains(&inacc_dir2));
        assert!(cached_fxtr.stripped_files.contains(&inacc_file));

        Ok(())
    }

    #[tokio::test]
    async fn test_fixture_create_and_cleanup() -> Result<(), TestSetupError> {
        let ctx = TestContext::new()?;
        prepare_dirs(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDirs(err))?;

        let mut fxtr_context = FixturesContext {
            fixture_path: ctx.config_mock.media.test_fixtures_path.clone(),
            stripped_dirs: Vec::new(),
            stripped_files: Vec::new(),
            fixtures_cache_path: ctx.config_mock.media.test_fixtures_path.join("fixtures_state.json")
        };

        prepare_fixtures(&mut fxtr_context).map_err(|err| TestSetupError::FaileToPrepareFixtures(err))?;
        assert_eq!(fxtr_context.stripped_dirs.len(), 2);
        
        for dir_path in fxtr_context.stripped_dirs {
            assert!(dir_path.exists());
        }

        assert!(fxtr_context.fixtures_cache_path.exists());

        cleanup(&fxtr_context.fixtures_cache_path).map_err(|err| TestSetupError::FailedToCleanThingsUp{ source: err, fixtures_path: fxtr_context.fixture_path.to_path_buf()})?;

        assert!(!fxtr_context.fixture_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_ffmpeg_download_and_unzip() -> Result<(), TestSetupError> {
        use httpmock::MockServer;
        let server = MockServer::start();

        let mut ctx = TestContext::new()?;
        prepare_dirs(&ctx.config_mock).map_err(|err| TestSetupError::FailedToPrepareDirs(err))?;

        let dummy_ffmpeg_path = ctx.tempdir.path().join("ffmpeg.exe");

        let mut dummy_ffmpeg_exe = File::create(&dummy_ffmpeg_path)?;
        dummy_ffmpeg_exe.write("hello world!".as_bytes())?;

        let dummy_zip_path = ctx.tempdir.path().join("ffmpeg.7z");
        sevenz_rust2::compress_to_path(&dummy_ffmpeg_path, &dummy_zip_path).map_err(|err| TestSetupError::FailedToCompressDummy(err))?;

        let archive_bytes = File::open(dummy_zip_path)?.bytes().collect::<Result<Vec<u8>, _>>()?;
        server.mock(|when, then| {
            when.path("/ffmpeg.7z");
            then.status(200).body(archive_bytes.clone());
        });

        let mut hasher = Sha256::new();
        hasher.update(&archive_bytes);

        let hex = format!("{:x}", hasher.finalize());
        server.mock(|when, then| {
            when.path("/checksum");
            then.status(200).body(format!("{}", hex));
        });

        ctx.set_ffmpeg_dl_mirror(format!("{}/ffmpeg.7z", server.url("")));
        ctx.set_ffmpeg_sha_dl_mirror(format!("{}/checksum", server.url("")));

        prepare_ffmpeg(&ctx.config_mock).await.map_err(|err| TestSetupError::FailedToPrepareFfmpeg(err))?;

        assert!(ctx.config_mock.media.ffmpeg_exe_path.exists());

        let content = std::fs::read_to_string(&ctx.config_mock.media.ffmpeg_exe_path)?;
        assert_eq!(content, "hello world!");

        assert!(!ctx.config_mock.media.ffmpeg_dir_path.join("ffmpeg.7z").exists());

        Ok(())
}
}