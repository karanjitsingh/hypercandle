use anyhow::{Context, Result};
use aws_sdk_s3::Client;
use std::path::Path;

use crate::cache;
use crate::{DataSource, S3_BUCKET};

/// Fetch an hourly fills file from S3, using local cache if available.
pub async fn fetch_hourly(
    client: &Client,
    data_dir: &Path,
    date: &str,
    hour: u8,
    source: DataSource,
) -> Result<Vec<u8>> {
    if let Some(path) = cache::get_cached(data_dir, date, hour, source) {
        eprintln!("cache hit: {date}/{hour}.lz4");
        return std::fs::read(&path).context("reading cached file");
    }

    let prefix = source.s3_prefix();
    let key = format!("{prefix}/{date}/{hour}.lz4");
    eprintln!("downloading: s3://{S3_BUCKET}/{key}");

    let resp = client
        .get_object()
        .bucket(S3_BUCKET)
        .key(&key)
        .request_payer(aws_sdk_s3::types::RequestPayer::Requester)
        .send()
        .await
        .with_context(|| format!("fetching s3://{S3_BUCKET}/{key}"))?;

    let data = resp
        .body
        .collect()
        .await
        .context("reading S3 response body")?
        .into_bytes()
        .to_vec();

    cache::write_cache(data_dir, date, hour, source, &data)?;
    Ok(data)
}

/// Create an S3 client using default credential chain.
pub async fn create_client() -> Client {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new("ap-northeast-1"))
        .load()
        .await;
    Client::new(&config)
}
