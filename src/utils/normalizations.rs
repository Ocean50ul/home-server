use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

pub fn normalize_name(name: &str) -> String {
    name
        .trim()
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect()
}

pub fn normalize_path(path: &Path) -> PathBuf {
    path.to_string_lossy()
        .to_lowercase()
        .replace('\\', "/")
        .into()
}