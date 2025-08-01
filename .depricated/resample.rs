// DEPRICATED
// rewrote it.

use std::{fs, io::{self, Write}, path::{Path, PathBuf}, sync::{Arc, Mutex}};
use anyhow::{Error, anyhow};
use rayon::prelude::*;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::get_probe;
use symphonia::core::probe::Hint;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ResampleStats {
    total: usize,
    processed: Arc<AtomicUsize>,
    resampled: Arc<AtomicUsize>,
    output_lock: Arc<Mutex<()>>
}

impl ResampleStats {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            processed: Arc::new(AtomicUsize::new(0)),
            resampled: Arc::new(AtomicUsize::new(0)),
            output_lock: Arc::new(Mutex::new(()))
        }
    }

    pub fn print_report(&self) -> Result<(), Error> {
        println!(
            "\nResampled {} / {}", self.resampled.load(Ordering::SeqCst),
            self.total
        );

        Ok(())
    }
}

impl AudioFormat {
    pub fn get_target_sample_rate(&self) -> u32 {
        match self {
            AudioFormat::Flac => {88200},
            AudioFormat::Mp3 => {44100},
            AudioFormat::Wav => {88200}
        }
    }
}

pub fn needs_resample(file: &AudioFile) -> Result<bool, Error> {
    let path = &file.path;
    let target_rate = file.format.get_target_sample_rate();
    
    match get_sample_rate(path)? {
        Some(sample_rate) => Ok(sample_rate > target_rate),
        None => Err(anyhow!("Error retrieving sample rate out of {:?}", path))
    }
}

pub fn get_sample_rate(path: &PathBuf) -> Result<Option<u32>, Error> {
    let hint = Hint::new();
    let src = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(src), MediaSourceStreamOptions::default());

    let probed = get_probe()
        .format(&hint, mss, &Default::default(), &Default::default())?;
    let format = probed.format;

    if let Some(track) = format.default_track() {
        Ok(track.codec_params.sample_rate)
    } else {
        Err(anyhow!("Failed to retrieve sample rate of a {:?}", path))
    }
    
}

pub fn resample_audio(audio_file: &AudioFile) -> Result<(), Error> {
    match audio_file.format {
        AudioFormat::Flac => resample_flac(&audio_file.path),
        AudioFormat::Mp3 => Err(anyhow!("Mp3 format resampling is not implemented yet!")),
        AudioFormat::Wav => Err(anyhow!("WAV format resampling is not implemented yet!"))
    }
}

pub fn resample_flac(inpt_path: &PathBuf) -> Result<(), Error> {
    let ffmpeg_path = Path::new("C:/Users/OceanSoul/Desktop/WEB_Rust/home-server_axum/ffmpeg/bin/ffmpeg.exe");
    let inpt_path_str = inpt_path.to_str().ok_or(anyhow!("Error converting input path into string: {:?}", inpt_path))?;
    let inpt_filename = inpt_path.file_name()
        .ok_or(anyhow!("Error retrieving file_name from a path: {:?}", inpt_path))?
        .to_str().ok_or(anyhow!("Error converting input path into string: {:?}", inpt_path))?;

    let temp = format!("C:/Users/OceanSoul/Desktop/WEB_Rust/home-server_axum/data/media/music/{}.temp.flac", inpt_filename);
    let status = Command::new(ffmpeg_path)
        .args([
            "-i", inpt_path_str,
            "-ar", "88200",
            "-c:a", "flac",
            &temp
        ])
        .status()?;

    if status.success() {
        fs::rename(temp, inpt_path)?;
        return Ok(());
    } else {
        return Err(anyhow!("ffmpeg exited with status: {}", status));
    }
}

pub fn update_progress_bar(stats: &ResampleStats) -> Result<(), Error> {
    let current = stats.processed.load(Ordering::SeqCst);
    let percent = (current * 100) / stats.total;
    let bar_width = 20;
    let filled = if percent > 0 { (percent * bar_width) / 100 } else { 0 };

    let bar = format!("[{}>{}]", "=".repeat(filled), ".".repeat(bar_width - filled));
    let output_string = format!("{} {}%", bar, percent.to_string());

    let _lock = stats.output_lock.lock().map_err(|e| anyhow!("Failed to lock empty mutex for print: {}", e))?;
    print!("\r{}", output_string);
    io::stdout().flush().map_err(|e| anyhow!("Failed to flush output stream: {}", e))?;

    Ok(())
}

pub fn resample_audio_library() -> Result<(), Error> {
    let formats = [AudioFormat::Flac];
    let files = get_audio_files(&formats)?;

    let stats = ResampleStats::new(files.len());

    println!("\nResampling tracks..");

    files.par_iter()
        .try_for_each(|audio_file| {

            if needs_resample(audio_file)? {
                resample_audio(audio_file)?;
                stats.resampled.fetch_add(1, Ordering::SeqCst);
            }

            stats.processed.fetch_add(1, Ordering::SeqCst);
            update_progress_bar(&stats)?;

            Ok::<(), Error>(())
        })?;
    
    update_progress_bar(&stats)?;
    println!("");
    stats.print_report()?;

    Ok(())
}
