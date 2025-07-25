use std::{collections::HashMap, env::{self, VarError}, fs, path::{Path, PathBuf}, process::Command};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::domain::audiofile::AudioFileType;

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
        
        fs::write(&self.fixtures_cache_path, json_str.as_bytes())?;
        
        Ok(())
    }

    pub fn cache_exists(&self) -> bool {
        self.fixtures_cache_path.exists()
    }
}

pub fn make_inaccessible_dir(name: &str, fctx: &mut FixturesContext) -> Result<PathBuf, FixturesSetupError> {
    let dir_path = fctx.fixture_path.join(name);

    fs::create_dir(&dir_path)?;
    strip_permissions(&dir_path)?;

    // Track for cleanup
    fctx.stripped_dirs.push(dir_path.clone());

    Ok(dir_path)
}

pub fn make_inaccessable_file(path: &Path, fctx: &mut FixturesContext) -> Result<(), FixturesSetupError> {
    fs::write(path, b"test")?;
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

    fs::create_dir_all(fctx.fixture_path.join("/files"))?;
    fs::create_dir_all(fctx.fixture_path.join("/dirs/inaccessible_dir"))?;

    fs::create_dir_all(fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    strip_permissions(&fctx.fixture_path.join("/dirs/inaccessible_dir/"))?;
    strip_permissions(&fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    create_audio_files()?;

    fctx.cache()?;

    Ok(())
}

pub fn create_audio_files() -> Result<(), FixturesSetupError> {
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
    let json_str = fs::read_to_string(PathBuf::from("./test_fixtures/fixtures_state.json"))?;
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

    fs::remove_dir_all(fctx.fixture_path)?;

    Ok(())
}