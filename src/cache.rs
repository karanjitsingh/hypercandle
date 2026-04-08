use anyhow::Result;
use std::path::{Path, PathBuf};

/// Returns the cache path for a given S3 key, creating parent dirs as needed.
/// Cache layout: {cache_dir}/{date}/{hour}.lz4
pub fn cache_path(cache_dir: &Path, date: &str, hour: u8) -> PathBuf {
    cache_dir.join(date).join(format!("{hour}.lz4"))
}

/// Check if a cached file exists and return its path.
pub fn get_cached(cache_dir: &Path, date: &str, hour: u8) -> Option<PathBuf> {
    let p = cache_path(cache_dir, date, hour);
    p.exists().then_some(p)
}

/// Write bytes to cache, creating directories as needed.
pub fn write_cache(cache_dir: &Path, date: &str, hour: u8, data: &[u8]) -> Result<PathBuf> {
    let p = cache_path(cache_dir, date, hour);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, data)?;
    Ok(p)
}
