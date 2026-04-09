//! Local file cache for downloaded S3 objects.
//!
//! Raw LZ4 files are stored as-is (no decompression) at:
//!   `data/s3/hl-mainnet-node-data/{prefix}/{date}/{hour}.lz4`
//!
//! This mirrors the S3 key structure so each source format gets its own
//! directory, avoiding collisions between legacy and current formats.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::{DataSource, S3_BUCKET};

/// Returns the local cache path for a given S3 hourly file.
pub fn cache_path(data_dir: &Path, date: &str, hour: u8, source: DataSource) -> PathBuf {
    data_dir
        .join("s3")
        .join(S3_BUCKET)
        .join(source.s3_prefix())
        .join(date)
        .join(format!("{hour}.lz4"))
}

/// Check if a cached file exists and return its path.
pub fn get_cached(data_dir: &Path, date: &str, hour: u8, source: DataSource) -> Option<PathBuf> {
    let p = cache_path(data_dir, date, hour, source);
    p.exists().then_some(p)
}

/// Write raw bytes to cache, creating directories as needed.
pub fn write_cache(
    data_dir: &Path,
    date: &str,
    hour: u8,
    source: DataSource,
    data: &[u8],
) -> Result<PathBuf> {
    let p = cache_path(data_dir, date, hour, source);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, data)?;
    Ok(p)
}
