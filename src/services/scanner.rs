use std::{ffi::OsStr, fs::File, io::BufReader, path::{Path, PathBuf}};

use lofty::probe::Probe;
use walkdir::WalkDir;

use super::{ScanError};
use crate::{domain::audiofile::{AudioFileType, AudioFileDescriptor, AudioFileMetadata}};

pub struct MediaScanner {
    music_lib_path: PathBuf,
}

impl MediaScanner {

    // TODO: Make it to be &'static PathBuf?
    pub fn new<P: AsRef<Path>>(music_path: P) -> Self {
        Self {
            music_lib_path: music_path.as_ref().to_owned(),
        }
    }

    // right now this function is synchronous, which is not ideal
    // TODO: make it async with tokio::fs
    pub fn scan_music_lib(&self) -> Result<ScanResult, ScanError> {

        // A quick check to fail fast if the root directory is inaccessible.
        // The error here is fatal and will halt the scan.
        std::fs::read_dir(&self.music_lib_path)
            .map_err(|e| ScanError::RootDirAccessError {
                path: self.music_lib_path.display().to_string(),
                source: e,
            })?;

        let walker = WalkDir::new(&self.music_lib_path).min_depth(1);
        let mut scan_result = ScanResult::new();
        
        // Iterate over every file and directory.
        // Errors encountered here are soft and being collected to return alongside with the successful results.
        for entry_result in walker {
            
            match entry_result {
                Err(err) => {
                    scan_result.errors.push(ScanError::WalkdirError(err));
                },
                Ok(dir_entry) => {
                    let path = dir_entry.path();

                    if path.is_dir() || path.is_symlink() {
                        log::warn!("Skipping {:?} since its either dir or symlink.", path);
                        continue;
                    }

                    if !self.is_audio_file(path) {
                        log::warn!("Skipping file with unsupported extension: {}", self.prettify_path(&path));
                        continue;
                    }

                    match self.process_file(path) {
                        Ok(descriptor) => {
                            scan_result.descriptors.push(descriptor);
                        },
                        Err(err) => {
                            log::warn!("Skipping file {}: {}", self.prettify_path(&path), err);
                            scan_result.errors.push(ScanError::IOError(err));
                            continue;
                        }
                    }

                }
            }
        }

        Ok(scan_result)
    }

    fn is_audio_file(&self, path: &Path) -> bool {
        path.extension()
            .map(|ext| AudioFileType::is_supported_extension(ext))
            .unwrap_or(false)
    }

    fn process_file(&self, path: &Path) -> Result<AudioFileDescriptor, std::io::Error> {
        // file access denied error propagating here, below, when you try to open the file
        let file = File::open(path)?;
        
        let file_size = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(err) => {
                log::warn!("Failed to access metadata for {}: {}. Setting file_size to 0.", self.prettify_path(&path), err);
                0u64
            }
        };
        
        let reader = BufReader::new(file);
        Ok(self.make_descriptor(path, file_size, reader))
    }

    fn type_from_ext(&self, path: &Path) -> AudioFileType {
        let extension = path.extension()
            .unwrap_or_else(|| {
                log::warn!("Failed to extract extension from path. Extension is unknown for {}", self.prettify_path(&path));
                OsStr::new("unknown")
            });
        
        AudioFileType::from_os_ext(extension)
    }

    fn extract_type_and_metadata(&self, path: &Path, reader: &mut BufReader<File>) -> (AudioFileType, AudioFileMetadata) {
        match Probe::new(reader).guess_file_type() {
            Ok(probe) => {

                // if lofty has failed to determine the type, we fall back to guessing from the extension.
                let file_type = probe.file_type()
                    .map(|ft| AudioFileType::from_lofty(&ft))
                    .unwrap_or_else(|| self.type_from_ext(path));
                
                // if probe.read() fails, then metadata falls back to default values
                let metadata = AudioFileMetadata::extract_or_default(probe.read());
                
                (file_type, metadata)
            },
            Err(err) => {
                // if probe has failed, we fall back to default values
                log::warn!("Failed to probe {}: {}", self.prettify_path(&path), err);
                (self.type_from_ext(path), AudioFileMetadata::default())
            }
        }
    }

    fn make_descriptor(&self, path: &Path, file_size: u64, mut reader: BufReader<File>) -> AudioFileDescriptor {
        let (file_type, metadata) = self.extract_type_and_metadata(path, &mut reader);
    
        AudioFileDescriptor {
            path: path.to_path_buf(),
            file_size,
            file_type,
            metadata
        }

    }

    fn prettify_path(&self, path: &Path) -> String {
        let base_dir = &self.music_lib_path;
    
        path.strip_prefix(&base_dir)
            .map(|path_suffix| {
                format!("./{}", path_suffix.display())
            })
            .unwrap_or_else(|_| path.to_path_buf().to_string_lossy().to_string())
    }
}

#[derive(Debug)]
pub struct ScanResult {
    pub descriptors: Vec<AudioFileDescriptor>,
    pub errors: Vec<ScanError>,
}

impl ScanResult {
    fn new() -> Self {
        Self {
            descriptors: Vec::new(),
            errors: Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs, os::windows::fs::{symlink_dir, symlink_file}, path::{Path, PathBuf}};

    use tempfile::{tempdir_in, TempDir};
    use walkdir::WalkDir;

    use crate::{services::test_helpers::*};
    use super::*;

    struct TestContext {
        temp_dir: TempDir,
        fixtures: Vec<PathBuf>
    }

    impl TestContext {
        async fn new() -> Result<Self, TestSetupError> {
            Ok(
                Self {
                    temp_dir: tempfile::tempdir()?,
                    fixtures: Vec::new()
                }
            )
        }

        fn normalize_path(&self, path: &Path) -> PathBuf {
            path.to_string_lossy()
                .to_lowercase()
                .replace('\\', "/")
                .into()
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
                new_paths.push(self.normalize_path(&dest));
            }

            self.fixtures = new_paths;
            Ok(self)
        }
    }

    fn assert_no_metadata(metadata: &AudioFileMetadata) -> () {
        let default_metadata = AudioFileMetadata::default();

        assert_eq!(metadata.artist_name, default_metadata.artist_name);
        assert_eq!(metadata.album_name, default_metadata.album_name);
        assert_eq!(metadata.track_name, default_metadata.track_name);
    }

    fn assert_some_metadata(metadata: &AudioFileMetadata) -> () {
        let default_metadata = AudioFileMetadata::default();

        assert!(metadata.album_name != default_metadata.album_name);
        assert!(metadata.artist_name != default_metadata.artist_name);
        assert!(metadata.track_name != default_metadata.artist_name);
    }

    #[tokio::test]
    async fn test_scan_empty_folder() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?;
        let scanner = MediaScanner::new(ctx.temp_dir.path());

        let scan_result = scanner.scan_music_lib()?;

        assert!(scan_result.descriptors.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_path_doesnt_exist() -> Result<(), TestSetupError> {
        init_logger()?;

        let scanner = MediaScanner::new(PathBuf::from("C:/path/doesnt/exist"));
        let scan_result = scanner.scan_music_lib();
        assert!(scan_result.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_non_audio_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?;
        let _temp_files = create_temp_files(ctx.temp_dir.path(), 1, "txt")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(scan_result.descriptors.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_shallow_mp3_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?;
        let _temp_files = create_temp_files(ctx.temp_dir.path(), 1, "mp3")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());

        assert!(matches!(scan_result.descriptors[0].file_type, AudioFileType::Mp3));
        assert_no_metadata(&scan_result.descriptors[0].metadata);

        Ok(())

    }

    #[tokio::test]
    async fn test_scan_shallow_flac_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?;
        let _temp_files = create_temp_files(ctx.temp_dir.path(), 1, "flac")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());

        assert!(matches!(scan_result.descriptors[0].file_type, AudioFileType::Flac));
        assert_no_metadata(&scan_result.descriptors[0].metadata);

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_shallow_wav_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?;
        let _temp_files = create_temp_files(ctx.temp_dir.path(), 1, "wav")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());

        assert!(matches!(scan_result.descriptors[0].file_type, AudioFileType::Wav));
        assert_no_metadata(&scan_result.descriptors[0].metadata);

        Ok(())
    }

    #[tokio::test]
    async fn tests_scan_vaild_mp3_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::Mp3ValidMetadata])?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());
        assert_some_metadata(&scan_result.descriptors[0].metadata);

        Ok(())
    }

    #[tokio::test]
    async fn tests_scan_vaild_wav_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::WavValidMetadata])?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());
        assert_some_metadata(&scan_result.descriptors[0].metadata);

        Ok(())
    }

    #[tokio::test]
    async fn tests_scan_vaild_flac_file() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::FlacValidMetadata])?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());
        assert_some_metadata(&scan_result.descriptors[0].metadata);

        Ok(())
    }

    // #[tokio::test]
    // async fn test_scan_mp3_no_metadata() -> Result<(), TestSetupError> {
    //     init_logger()?;

    //     let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::Mp3NoMetadata])?;

    //     let scanner = MediaScanner::new(ctx.temp_dir.path());
    //     let scan_result = scanner.scan_music_lib()?;

    //     assert!(!scan_result.descriptors.is_empty());
    //     assert_no_metadata(&scan_result.descriptors[0].metadata);

    //     Ok(())
    // }

    // #[tokio::test]
    // async fn test_scan_mp3_corrupted_header() -> Result<(), TestSetupError> {
    //     init_logger()?;

    //     let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::Mp3CorruptedHeader])?;

    //     let scanner = MediaScanner::new(ctx.temp_dir.path());
    //     let scan_result = scanner.scan_music_lib()?;

    //     assert!(!scan_result.descriptors.is_empty());
    //     assert_no_metadata(&scan_result.descriptors[0].metadata);

    //     Ok(())
    // }

    #[tokio::test]
    async fn test_scan_multiple_files() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::Mp3ValidMetadata, FixtureFileNames::FlacValidMetadata])?;
        let _temp_files = create_temp_files(ctx.temp_dir.path(), 1, "txt")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert!(!scan_result.descriptors.is_empty());
        assert_eq!(scan_result.descriptors.len(), 2);

        for audio_file in scan_result.descriptors {
            assert_some_metadata(&audio_file.metadata);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_nested_dirs() -> Result<(), TestSetupError> {
        init_logger()?;

        let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::FlacValidMetadata])?;

        let nested_1 = tempdir_in(&ctx.temp_dir)?;
        let nested_2 = tempdir_in(&ctx.temp_dir)?;
        let sub_nested_2 = tempdir_in(&nested_2)?;

        let _temp_file_n1 = create_temp_files(nested_1.path(), 1, "txt")?;
        let _temp_file_subn2 = create_temp_files(sub_nested_2.path(), 1, "mp3")?;

        let scanner = MediaScanner::new(ctx.temp_dir.path());
        let scan_result = scanner.scan_music_lib()?;

        assert_eq!(scan_result.descriptors.len(), 2);

        for f_descr in scan_result.descriptors {
            match f_descr.file_type {
                AudioFileType::Flac => {
                    assert_some_metadata(&f_descr.metadata);
                },
                AudioFileType::Mp3 => {
                    assert_no_metadata(&f_descr.metadata);
                },
                _ => {}
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_unicode_filenames() -> Result<(), TestSetupError> {
        init_logger()?;

        let unicode_filenames = vec![
            "音楽.mp3",
            "Björk - Jóga.flac",
            "приветмедвед.wav",
            "🎵 My Jam 🎵.mp3"
        ];

        let ctx = TestContext::new().await?;
        let scan_dir = ctx.temp_dir.path();

        for filename in &unicode_filenames {
            let file_path = scan_dir.join(filename);
            fs::write(file_path, "dummy data")?;
        }

        let scanner = MediaScanner::new(scan_dir);
        let scanner_result = scanner.scan_music_lib()?;

        assert_eq!(scanner_result.descriptors.len(), unicode_filenames.len());

        let found_paths: HashSet<PathBuf> = scanner_result.descriptors
            .into_iter()
            .map(|d| d.path)
            .collect();

        for filename in unicode_filenames {
            let expected_path = &scan_dir.join(filename);
            assert!(found_paths.contains(expected_path));
        }

        Ok(())

    }

    #[tokio::test]
    async fn test_scan_special_chars_in_path() -> Result<(), TestSetupError> {
        init_logger()?;

        let unicode_filenames = vec![
           "AC!DC - T.N.T..mp3",
            "(I Can't Get No) Satisfaction & Paint It Black.mp3",
            "  A Spacey Song  .flac"
        ];

        let ctx = TestContext::new().await?;
        let scan_dir = ctx.temp_dir.path();

        for filename in &unicode_filenames {
            let file_path = scan_dir.join(filename);
            fs::write(file_path, "dummy data")?;
        }

        let scanner = MediaScanner::new(scan_dir);
        let scanner_result = scanner.scan_music_lib()?;

        assert_eq!(scanner_result.descriptors.len(), unicode_filenames.len());

        let found_paths: HashSet<PathBuf> = scanner_result.descriptors
            .into_iter()
            .map(|d| d.path)
            .collect();

        println!("{:?}", found_paths);
        for filename in unicode_filenames {
            let expected_path = scan_dir.join(filename);
            assert!(found_paths.contains(&expected_path));
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    mod windows_tests {
        use super::*;

        #[tokio::test]
        async fn test_scan_symlink_to_file() -> Result<(), TestSetupError> {
            init_logger()?;

            let ctx = TestContext::new().await?.with_fixtures(&[FixtureFileNames::FlacValidMetadata])?;
            let target_file_path = &ctx.fixtures[0];

            let scan_dir = ctx.temp_dir.path().join("music_library");
            fs::create_dir(&scan_dir)?;

            let symlink_path = scan_dir.join("my_music_link.flac");
            symlink_file(target_file_path, &symlink_path)?;

            let scanner = MediaScanner::new(scan_dir);
            let scanner_result = scanner.scan_music_lib()?;

            assert_eq!(scanner_result.descriptors.len(), 0);

            Ok(())
        }

        #[tokio::test]
        async fn test_scan_symlink_to_directory() -> Result<(), TestSetupError> {
            init_logger()?;

            let ctx = TestContext::new().await?;

            let subdir_path = ctx.temp_dir.path().join("album1");
            fs::create_dir(&subdir_path)?;

            let symlink_path = ctx.temp_dir.path().join("symlink_to_dir");
            symlink_dir(&subdir_path, &symlink_path)?;

            let scanner = MediaScanner::new(ctx.temp_dir.path());
            let scanner_result = scanner.scan_music_lib()?;

            assert_eq!(scanner_result.descriptors.len(), 0);

            Ok(())
        }

        #[tokio::test]
        async fn test_scan_broken_symlink() -> Result<(), TestSetupError> {
            init_logger()?;

            let ctx = TestContext::new().await?;

            let symlink_path = ctx.temp_dir.path().join("symlink.flac");
            let path_to_nowhere = PathBuf::from("D:/path/to/nowhere");
            symlink_file(path_to_nowhere, &symlink_path)?;

            let scanner = MediaScanner::new(ctx.temp_dir.path());
            let scanner_result = scanner.scan_music_lib()?;

            assert!(scanner_result.descriptors.len() == 0);

            Ok(())
        }

        #[tokio::test]
        async fn test_scan_circular_symlink() -> Result<(), TestSetupError> {
            init_logger()?;

            let ctx = TestContext::new().await?;

            let subdir_path = ctx.temp_dir.path().join("album1");
            fs::create_dir(&subdir_path)?;

            let circular_link_path = subdir_path.join("link_to_root");
            symlink_dir(ctx.temp_dir.path(), &circular_link_path)?;

            let scanner = MediaScanner::new(ctx.temp_dir.path());
            let scanner_result = scanner.scan_music_lib()?;

            assert!(scanner_result.descriptors.len() == 0);
            
            Ok(())
        }

        // #[tokio::test]
        // async fn test_scan_acess_denied_soft() -> Result<(), TestSetupError> {
        //     init_logger()?;

        //     let soft_deny_path = PathBuf::from(r"C:\Users\OceanSoul\Desktop\WEB_Rust\home-server_axum\tests\dirs\soft_deny");
        //     let scanner = MediaScanner::new(soft_deny_path);
        //     let scan_result = scanner.scan_music_lib()?;

        //     assert_eq!(scan_result.descriptors.len(), 1);
        //     assert_eq!(scan_result.errors.len(), 1);


        //     Ok(())
        // }

        // #[tokio::test]
        // async fn test_scan_acess_denied_hard() -> Result<(), TestSetupError> {
        //     init_logger()?;

        //     let hard_deny_path = PathBuf::from(r"C:\Users\OceanSoul\Desktop\WEB_Rust\home-server_axum\tests\dirs\hard_deny");
        //     let scanner = MediaScanner::new(&hard_deny_path);

        //     let scan_result = scanner.scan_music_lib();
        //     assert!(scan_result.is_err());

        //     let scan_error = scan_result.unwrap_err();

        //     match scan_error {
        //         ScanError::RootDirAccessError {path, ..} => {
        //             assert_eq!(path, hard_deny_path.to_string_lossy().to_string());
        //         },
        //         other => panic!("ScanError expected, but found: {}", other)
        //     }
        //     Ok(())
        // }

        // #[tokio::test]
        // async fn test_scan_file_access_denied() -> Result<(), TestSetupError> {
        //     init_logger()?;

        //     let file_deny_path = PathBuf::from(r"C:\Users\OceanSoul\Desktop\WEB_Rust\home-server_axum\tests\dirs\deny_file");
        //     let scanner = MediaScanner::new(&file_deny_path);

        //     let scan_result = scanner.scan_music_lib()?;

        //     assert_eq!(scan_result.descriptors.len(), 0);
        //     assert_eq!(scan_result.errors.len(), 1);

        //     Ok(())
        // }
    }
}