/*

We need a setup mechanism that prepares the development or production environment automatically, so contributors and users can run the server with minimal manual steps.

This service should handle:

    FFmpeg binary:
        Check if ffmpeg.exe exists (ffmpeg/ffmpeg.exe)
        If not, download it from the official source
        Verify checksum before using

    Database:
        Check if the local database file exists (data/db/database.db)
        If not, create the database and run necessary migrations (data/db/migrations/)

    Directory structure:
        Ensure all required directories exist (data/media/music, data/media/videos, etc.)
        Skip if services already create them automatically - investigate this

    Test fixtures:
        Create or verify necessary test fixture files or directories
        Caveat: part of the fixtures are file and two dirs with stripped permissions. Existence checks might be non-trivial - investigate this

The service should be run via CLI: e.g. cargo run prepare.

*/

use std::{collections::HashMap, env::{self, VarError}, fs::{create_dir, create_dir_all, read_to_string, remove_dir_all, remove_file, write, File}, io::{copy, Read}, path::{Path, PathBuf}, process::Command};

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use sevenz_rust2::{self, default_entry_extract_fn};

use crate::{domain::audiofile::AudioFileType, utils::config::{get_config, ConfigLoadingError}};

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

    #[error("Could not create dir '{path}': {source}")]
    DirCreateError { path: PathBuf, #[source] source: std::io::Error },
    
    #[error("Failed to extract ffmpeg.exe from archive: {0}")]
    ErrorExtractingFfmpeg(sevenz_rust2::Error),

    #[error(transparent)]
    FixtureSetupError(#[from] FixturesSetupError)
}

/* ======================= FFMPEG PREPARATION PART ======================= */

fn ffmpeg_exists(path: &Path) -> bool {
    path.exists()
}

fn download_ffmpeg_zip_essentials(dest_file_path: &Path, url: &str) -> Result<(), PrepareServiceError> {
    let mut dest_file = File::create(dest_file_path)
        .map_err(|err| PrepareServiceError::ErrorCreatingDestinationFile(err))?;

    let mut response = reqwest::blocking::get(url)?.error_for_status()?;

    copy(&mut response, &mut dest_file)
        .map_err(|err| PrepareServiceError::ErrorCopyingIntoDestinationFile(err))?;

    Ok(())
}

fn parse_checksum_html(html_string: &str) -> Result<String, PrepareServiceError> {
    Ok(
        html_string
            .split("<pre>")
            .nth(1)
            .and_then(|s| s.split("</pre>").next())
            .map(|s| s.trim().to_string())
            .ok_or(PrepareServiceError::FailedToParseChecksums())?
    )
}

fn get_checksums(checksum_url: &str) -> Result<String, PrepareServiceError> {
    let response = reqwest::blocking::get(checksum_url)?.error_for_status()?;
    let html_string = response.text()?;

    parse_checksum_html(&html_string)
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

fn unzip_ffmpeg(zip_path: &Path, file_name: &str, unzip_dest: &Path) -> Result<(), PrepareServiceError> {
    let src_reader = File::open(zip_path).map_err(|err| PrepareServiceError::FileOpenError{path: zip_path.to_path_buf(), source: err})?;

    sevenz_rust2::decompress_with_extract_fn(
        src_reader, 
        unzip_dest,
        |entry, reader, dest| {
            if entry.name() == file_name {
                let r = default_entry_extract_fn(entry, reader, dest);
                r
            } else {
                Ok(false)
            }
        }
    )
    .map_err(|err| PrepareServiceError::ErrorExtractingFfmpeg(err))?;

    Ok(())
}

pub fn prepare_ffmpeg() -> Result<(), PrepareServiceError> {
    let config = get_config()?;
    let ffmpeg_exe_path = &config.media.ffmpeg_exe_path;

    if ffmpeg_exists(&ffmpeg_exe_path) {
        return Ok(());
    }
    let zip_path =config.media.ffmpeg_dir_path.join(FFMPEG_ARCHIVE_NAME);
    let gyan_mirror = &config.media.ffmpeg_donwload_mirror;
    download_ffmpeg_zip_essentials(&zip_path, gyan_mirror)?;

    let checksum_url = &config.media.ffmpeg_sha_download_mirror;
    let expected_checksum = get_checksums(checksum_url)?;
    verify_checksums(&zip_path, expected_checksum)?;

    unzip_ffmpeg(&zip_path, FFMPEG_EXECUTABLE_NAME, &config.media.ffmpeg_dir_path)?;

    if !ffmpeg_exists(&ffmpeg_exe_path) {
        return Err(PrepareServiceError::FfmpegDoesntExist())
    }

    remove_file(&zip_path).map_err(|err| PrepareServiceError::FileRemoveError{path: zip_path.to_path_buf(), source: err})?;

    Ok(())
}

/* ======================= END OF FFMPEG PREPARATION PART ======================= */




/* ======================= DB PREPARATION PART ======================= */
pub fn prepare_db() -> Result<(), PrepareServiceError> {
    let config = get_config()?;
    let db_path = &config.database.path;

    if db_path.exists() {
        return Ok(())
    }

    File::create(&db_path).map_err(|err| PrepareServiceError::FileCreateError {path: db_path.to_path_buf(), source: err})?;

    Ok(())
}
/* ======================= END DB PREPARATION PART ======================= */




/* ======================= DIRS PREPARATION PART ======================= */
pub fn prepare_dirs() -> Result<(), PrepareServiceError> {
    let config = get_config()?;

    let paths = vec![
        &config.media.resampled_music_path,
        &config.media.video_path,
        &config.media.video_path,
        &config.media.ffmpeg_dir_path,
        &config.media.test_fixtures_path
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
    pub fn new(audio_type: AudioFileType) -> Result<AudioFixture, FixturesSetupError> {
        let (path, metadata) = match audio_type {
            AudioFileType::Flac => (
                String::from("./test_fixtures/files/flac_valid_metadata.flac"),
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
                String::from("./test_fixtures/files/mp3_valid_metadata.mp3"),
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
                String::from("./test_fixtures/files/wav_valid_metadata.wav"),
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
    fixture_path: PathBuf,
    stripped_files: Vec<PathBuf>,
    stripped_dirs: Vec<PathBuf>,
    fixtures_cache_path: PathBuf
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

pub fn create_fixtures(fctx: &mut FixturesContext) -> Result<(), FixturesSetupError> {
    if fctx.fixtures_cache_path.exists() {
        // right now assume that if cache exist, then all the fixutres are also presented.
        return Ok(());
    }

    create_dir_all(fctx.fixture_path.join("/files"))?;
    create_dir_all(fctx.fixture_path.join("/dirs/inaccessible_dir"))?;

    create_dir_all(fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    strip_permissions(&fctx.fixture_path.join("/dirs/inaccessible_dir"))?;
    strip_permissions(&fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    create_fixture_audio_files()?;

    fctx.cache()?;

    Ok(())
}

pub fn create_fixture_audio_files() -> Result<(), FixturesSetupError> {
    let fixtures = vec![
        AudioFixture::new(AudioFileType::Flac)?,
        AudioFixture::new(AudioFileType::Mp3)?,
        AudioFixture::new(AudioFileType::Wav)?
    ];

    for fix in fixtures {
        let mut cmd = Command::new("./ffmpeg/ffmpeg.exe");

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

pub fn cleanup() -> Result<(), FixturesSetupError> {
    let json_str = read_to_string(PathBuf::from("./test_fixtures/fixtures_state.json"))?;
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

pub fn run_prepare_service() -> Result<(), PrepareServiceError> {
    prepare_dirs()?;
    prepare_db()?;
    prepare_ffmpeg()?;

    let mut fixtures_context = FixturesContext::new();
    create_fixtures(&mut fixtures_context)?;

    Ok(())
}