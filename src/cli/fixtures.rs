use std::{env::{self, VarError}, fs, path::{Path, PathBuf}, process::Command};

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
    IcaclsCommandError(String)
}

pub struct FixturesContext {
    fixture_path: PathBuf,
    stripped_files: Vec<PathBuf>,
    stripped_dirs: Vec<PathBuf>
}

impl FixturesContext {
    pub fn new() -> Self {
        Self {
            fixture_path: PathBuf::from("./test_fixtures"),
            stripped_files: Vec::new(),
            stripped_dirs: Vec::new()
        }
    }

    pub fn cache(&self) -> Result<(), FixturesSetupError> {
        let cache_path = self.fixture_path.join("cache.txt");

        for dir in &self.stripped_dirs {
            fs::write(&cache_path, dir.to_string_lossy().as_bytes())?;
        }

        for file in &self.stripped_files {
            fs::write(&cache_path, file.to_string_lossy().as_bytes())?;
        }

        Ok(())
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
    if fctx.fixture_path.join("cache.txt").exists() {
        return Ok(());
    }

    fs::create_dir(&fctx.fixture_path)?;

    fs::create_dir(fctx.fixture_path.join("/dirs"))?;
    fs::create_dir(fctx.fixture_path.join("/files"))?;

    fs::create_dir(fctx.fixture_path.join("/dirs/inaccessible_dir"))?;

    fs::create_dir(fctx.fixture_path.join("/dirs/accessible_dir"))?;
    fs::create_dir(fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    strip_permissions(&fctx.fixture_path.join("/dirs/inaccessible_dir/"))?;
    strip_permissions(&fctx.fixture_path.join("/dirs/accessible_dir/inaccessible_dir"))?;

    get_audio_files(&fctx.fixture_path.join("/files"));

    fctx.cache()?;

    Ok(())
}

fn get_audio_files(destination: &Path) -> () {

}

pub fn cleanup(fctx: &mut FixturesContext) -> () {
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
}