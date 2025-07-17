// DEPRICATED:
// going to use the name tracks have, since it makes things a bit easier and i dont have a good reason to rename everything for now.

use std::path::PathBuf;
use std::fs;
use anyhow::{Error, anyhow};
use super::{
    get_audio_files,
    AudioFormat,
};

fn get_filename(file: &PathBuf) -> Result<String, Error> {
    let os_filename = file.file_name().ok_or(anyhow!("Error extracting file name out of {:?}", file))?;
    let str_filename = os_filename.to_str().ok_or(anyhow!("Error converting {:?} into str!", os_filename))?;

    Ok(str_filename.split('.').collect::<Vec<&str>>()[0].to_string())
}

fn get_extension(file: &PathBuf) -> Result<String, Error> {
    let os_extension = file.extension().ok_or(anyhow!("Error extracting file name out of {:?}", file))?;
    let str_extension = os_extension.to_str().ok_or(anyhow!("Error converting {:?} into str!", os_extension))?;

    Ok(str_extension.to_string())
}

fn is_alphanumeric(s: &str) -> bool {
    s.chars().all(|char| char.is_ascii_alphanumeric())
}

pub fn has_conventional_name(file: &PathBuf) -> Result<bool, Error> {
    let filename = get_filename(file)?;
    let extension = get_extension(file)?;

    Ok(
        is_alphanumeric(&filename) && is_alphanumeric(&extension)
        && filename.len() == 4
    )
    
}

fn highest_sequence_number(files: &Vec<PathBuf>) -> Result<u32, Error> {
    let mut highest = 0u32;

    for file in files {
        if !has_conventional_name(file)? { continue; }

        let file_name = get_filename(file)?;
        let parsed_name = file_name.parse::<u32>()?;

        if parsed_name > highest {
            highest = parsed_name;
        }
    }

    Ok(highest)
}

pub fn rename_on_demand() -> Result<(), Error> {
    let formats = [AudioFormat::Flac];
    let files: Vec<PathBuf> = get_audio_files(&formats)?.into_iter().map(|file| file.path).collect();
    let mut last = highest_sequence_number(&files)? + 1;
    let mut renamed_counter = 0;

    for file in files {
        if has_conventional_name(&file)? { continue; }

        let extension = get_extension(&file)?;
        let parent = file.parent().unwrap_or_else(|| std::path::Path::new("."));
        let new_path = parent.join(format!("{:04}.{}", last, extension));

        fs::rename(file, new_path)?;

        last += 1;
        renamed_counter += 1;
    }

    println!("Renamed {} tracks.", renamed_counter);
    Ok(())
}