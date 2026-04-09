use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::{DataSource, S3_BUCKET};

/// Returns the cache path for a given S3 key.
/// Layout: {data_dir}/s3/{bucket}/{prefix}/{date}/{hour}.lz4
pub fn cache_path(data_dir: &Path, date: &str, hour: u8, source: DataSource) -> PathBuf {
    data_dir
        .join("s3")
        .join(S3_BUCKET)
        .join(source.s3_prefix())
        .join(date)
        .join(format!("{hour}.lz4"))
}

pub fn get_cached(data_dir: &Path, date: &str, hour: u8, source: DataSource) -> Option<PathBuf> {
    let p = cache_path(data_dir, date, hour, source);
    p.exists().then_some(p)
}

pub fn write_cache(data_dir: &Path, date: &str, hour: u8, source: DataSource, data: &[u8]) -> Result<PathBuf> {
    let p = cache_path(data_dir, date, hour, source);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, data)?;
    Ok(p)
}
