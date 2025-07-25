use std::{path::{Path, PathBuf}, process::{Command, ExitStatus}};

use std::fs;
use rayon::{prelude::*, ThreadPoolBuildError, ThreadPoolBuilder};

use crate::{domain::audiofile::{AudioFileDescriptor, AudioFileType}, services::scanner::ScanResult};

#[derive(Clone, Debug, PartialEq)]
pub struct ResampleConfig {
    max_sample_rate: u32,
    strategy: ResampleStrategy,
    cache_dir: PathBuf,

    parallelism: ParallelismPolicy,

    // unsure whether i need those
    enable_backups: bool,
    supported_types: Vec<AudioFileType>
}

impl Default for ResampleConfig {
    fn default() -> Self {
        Self {
            max_sample_rate: 88200,
            strategy: ResampleStrategy::default(),
            cache_dir: PathBuf::from("./data/media/music/.resampled"),
            enable_backups: true,
            parallelism: ParallelismPolicy::default(),
            supported_types: Vec::new()
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParallelismPolicyError {
    #[error("reserved_fraction must be > 0.0 and < 1.0, got {0}")]
    ReservedFractionOutOfRange(f32),

    #[error("min_parallel cores must be > 0.0, got {0}")]
    NegativeOrZeroMinCores(usize)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParallelismPolicy {
    /// What fraction of logical cores to *reserve* for the rest of the system.
    /// (e.g. 0.3 means “leave 30% of threads free”)
    reserved_fraction: f32,

    /// If the machine has *fewer* than this many logical cores, always use 1 thread.
    min_parallel_cores: usize
}

impl Default for ParallelismPolicy {
    fn default() -> Self {
        Self {
            reserved_fraction: 0.3,
            min_parallel_cores: 5
        }
    }
}

impl ParallelismPolicy {
    pub fn new(reserved_fraction: f32, min_parallel_cores: usize) -> Result<Self, ParallelismPolicyError> {

        if reserved_fraction > 1.0 || reserved_fraction < 0.0 {
            return Err(ParallelismPolicyError::ReservedFractionOutOfRange(reserved_fraction));
        }

        if min_parallel_cores <= 0 {
            return Err(ParallelismPolicyError::NegativeOrZeroMinCores(min_parallel_cores))
        }

        Ok(
            Self {
                reserved_fraction,
                ..Default::default()
            }
        )
    }

    pub fn max_threads(&self) -> usize {
        let logical_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        if logical_cores < self.min_parallel_cores {
            1
        } else {
            let to_reserve = (logical_cores as f32 * self.reserved_fraction).ceil() as usize;
            logical_cores.saturating_sub(to_reserve).max(1)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum ResampleStrategy {
    InPlace,

    #[default]
    CopyToCache
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkipReason {
    FailedToRetrieveSampleRate,
    SampleRateLowerThanMax,
    InvalidPath
}

#[derive(Debug, thiserror::Error)]
pub enum ResampleError {
    #[error("Resample Service has encountered IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Resample Service has encountered an error while building thread pool: {0}")]
    ThreadPoolBuildError(#[from] ThreadPoolBuildError),

    #[error("Ffmpeg resampler has encountered an error and exited with: {0}")]
    FfmpegResamplerError(ExitStatus)
}

#[derive(Debug, Default)]
pub struct ResampleReport {
    processed_files: Vec<PathBuf>,
    skipped_files: Vec<(PathBuf, SkipReason)>,
    errors: Vec<(PathBuf, ResampleError)>
}

impl ResampleReport {
    pub fn new() -> Self {
        Self {
            processed_files: Vec::new(),
            skipped_files: Vec::new(),
            errors: Vec::new()
        }
    }
}

enum DescriptorOutcome {
    Processed(PathBuf),
    Skipped(PathBuf, SkipReason),
    Errored(PathBuf, ResampleError)
}

pub trait Resampler {
    fn resample(&self, input_path: &Path, output_path: &Path, file_type: &AudioFileType) -> Result<(), ResampleError>;
}

pub struct FfmpegResampler {
    ffmpeg_path: PathBuf
}

impl Resampler for FfmpegResampler {
    fn resample(&self, input_path: &Path, output_path: &Path, file_type: &AudioFileType) -> Result<(), ResampleError> {

        let inpt_path_str = input_path.to_string_lossy();
        let output_path_str = output_path.to_string_lossy();

        let status = Command::new(&self.ffmpeg_path)
            .args([
                "-i", &inpt_path_str,
                "-ar", &file_type.get_resample_target_rate().to_string(),
                "-c:a", file_type.as_str(),
                &output_path_str
            ])
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(ResampleError::FfmpegResamplerError(status))
        }

    }
}

pub struct ResampleService<R: Resampler> {
    config: ResampleConfig,
    resampler: R
}

impl<R: Resampler + Sync + Send> ResampleService<R> {
    pub fn new(config: ResampleConfig, resampler: R) -> Self {
        ResampleService { config, resampler }
    }

    pub fn resample_library(&self, scan_result: &ScanResult) -> Result<ResampleReport, ResampleError> {

        let pool = ThreadPoolBuilder::new()
            .num_threads(self.config.parallelism.max_threads())
            .build()?;


        // Do all the hard work in parallel.
        let outcomes: Vec<DescriptorOutcome> = pool.install(|| {
            scan_result
                .descriptors
                .par_iter()
                .map(|desc| self.handle_descriptor(desc))
                .collect()
        });

        // Make a report sequentially.
        let mut report = ResampleReport::new();

        for outcome in outcomes {
            match outcome {
                DescriptorOutcome::Processed(path)  => report.processed_files.push(path),
                DescriptorOutcome::Skipped(path, why)  => report.skipped_files.push((path, why)),
                DescriptorOutcome::Errored(path,err)  => report.errors.push((path, err)),
            }
        }

        Ok(report)
    }

    fn handle_descriptor(&self, descriptor: &AudioFileDescriptor) -> DescriptorOutcome {
        let path = &descriptor.path;

        let sample_rate = match descriptor.metadata.sample_rate {
            Some(sr) => sr,
            None => return DescriptorOutcome::Skipped(path.clone(), SkipReason::FailedToRetrieveSampleRate)
        };

        if sample_rate <= self.config.max_sample_rate {
            return DescriptorOutcome::Skipped(path.clone(), SkipReason::SampleRateLowerThanMax);
        }

        let file_name = match path.file_name() {
            Some(n) => n,
            None => return DescriptorOutcome::Skipped(path.clone(), SkipReason::InvalidPath)
        };

        let resample_outcome = match self.config.strategy {

            ResampleStrategy::CopyToCache => {
                let output_path = self.config.cache_dir.join(file_name);
                self.resampler.resample(&path, &output_path, &descriptor.file_type).map(|_| DescriptorOutcome::Processed(path.clone()))
            },

            ResampleStrategy::InPlace => {
                let tmp = self.config.cache_dir.join(file_name);

                match self.resampler.resample(&path, &tmp, &descriptor.file_type) {
                    Ok(()) => fs::rename(&tmp, path)
                        .map(|_| DescriptorOutcome::Processed(path.clone()))
                        .map_err(ResampleError::IOError),

                    Err(e) => Err(e)
                }
            }
        };

        match resample_outcome {
            Ok(o) => o,
            Err(err) => DescriptorOutcome::Errored(path.clone(), err)
        }
    }
}