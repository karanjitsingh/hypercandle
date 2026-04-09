//! Parse LZ4-compressed NDJSON fills into trades.
//!
//! Supports three data formats (see [`DataSource`]):
//! - **FillsByBlock**: blocks with `events` array of `[address, fill]` pairs
//! - **NodeFills**: flat `[address, fill]` per line (same fill schema)
//! - **NodeTrades**: trade objects with ISO timestamps, deduped by hash
//!
//! Fills appear twice in the data (once per counterparty). We deduplicate
//! by `tid` (or `hash` for NodeTrades) to avoid double-counting volume.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashSet;
use std::str::FromStr;
use tracing::instrument;

use crate::DataSource;

// -- FillsByBlock format: each line is a block --
#[derive(Debug, Deserialize)]
struct Block {
    events: Vec<(serde::de::IgnoredAny, Fill)>,
}

// -- Fill schema shared by FillsByBlock and NodeFills --
#[derive(Debug, Deserialize)]
struct Fill {
    coin: String,
    px: String,
    sz: String,
    side: String,
    time: u64, // epoch ms
    tid: u64,  // trade ID, used for deduplication
}

// -- NodeTrades format: different schema, ISO timestamps --
#[derive(Debug, Deserialize)]
struct NodeTrade {
    coin: String,
    px: String,
    sz: String,
    side: String,
    time: String, // ISO 8601 e.g. "2025-03-31T23:59:59.962208772"
    hash: String, // tx hash, used for deduplication (no tid in this format)
}

/// A normalized trade extracted from any of the three formats.
/// Uses `Decimal` for exact price/volume arithmetic.
#[derive(Debug, Clone)]
pub struct Trade {
    pub price: Decimal,
    pub size: Decimal,
    pub time_ms: u64,
    pub is_buy: bool,
}

/// Decompress LZ4 data and parse trades for the given coin.
/// Dispatches to the correct parser based on the data source.
#[instrument(skip(lz4_data), fields(coin, source = ?source))]
pub fn parse_fills(lz4_data: &[u8], coin: &str, source: DataSource) -> Result<Vec<Trade>> {
    let text = decompress(lz4_data)?;
    let mut trades = match source {
        DataSource::FillsByBlock => parse_fills_by_block(&text, coin)?,
        DataSource::NodeFills => parse_node_fills(&text, coin)?,
        DataSource::NodeTrades => parse_node_trades(&text, coin)?,
    };
    trades.sort_by_key(|t| t.time_ms);
    Ok(trades)
}

#[instrument(skip(lz4_data), fields(compressed_bytes = lz4_data.len()))]
fn decompress(lz4_data: &[u8]) -> Result<String> {
    use std::io::Read;
    let mut decoder = lz4::Decoder::new(lz4_data).context("creating LZ4 decoder")?;
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).context("LZ4 decompression")?;
    String::from_utf8(buf).context("invalid UTF-8")
}

/// Pre-filter: build a string pattern to quickly skip lines that can't
/// contain the target coin. Avoids deserializing ~88% of JSON for BTC.
fn coin_filter(coin: &str) -> String {
    format!("\"{}\"", coin)
}

/// Parse FillsByBlock format: each line is `{"events": [[addr, fill], ...]}`.
fn parse_fills_by_block(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let filter = coin_filter(coin);
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        if !line.contains(&filter) {
            continue;
        }
        let block: Block = serde_json::from_str(line).context("parsing block JSON")?;
        for (_addr, fill) in &block.events {
            if let Some(t) = extract_fill(fill, coin, &mut seen)? {
                trades.push(t);
            }
        }
    }
    Ok(trades)
}

/// Parse NodeFills format: each line is `[address, fill]`.
fn parse_node_fills(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let filter = coin_filter(coin);
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        if !line.contains(&filter) {
            continue;
        }
        let (_addr, fill): (serde::de::IgnoredAny, Fill) =
            serde_json::from_str(line).context("parsing node_fill JSON")?;
        if let Some(t) = extract_fill(&fill, coin, &mut seen)? {
            trades.push(t);
        }
    }
    Ok(trades)
}

/// Extract a Trade from a Fill, deduplicating by tid.
fn extract_fill(fill: &Fill, coin: &str, seen: &mut HashSet<u64>) -> Result<Option<Trade>> {
    if fill.coin != coin {
        return Ok(None);
    }
    // Each trade appears twice (once per counterparty). Skip duplicates.
    if fill.tid == 0 || !seen.insert(fill.tid) {
        return Ok(None);
    }
    let price = Decimal::from_str(&fill.px).context("parsing price")?;
    let size = Decimal::from_str(&fill.sz).context("parsing size")?;
    if size.is_zero() {
        return Ok(None);
    }
    Ok(Some(Trade {
        price,
        size,
        time_ms: fill.time,
        is_buy: fill.side == "B",
    }))
}

/// Parse NodeTrades format: each line is a trade object with ISO timestamp.
/// Already one trade per line (not duplicated per side), dedup by hash.
fn parse_node_trades(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let nt: NodeTrade = serde_json::from_str(line).context("parsing node_trade JSON")?;
        if nt.coin != coin || !seen.insert(nt.hash.clone()) {
            continue;
        }
        let price = Decimal::from_str(&nt.px).context("parsing price")?;
        let size = Decimal::from_str(&nt.sz).context("parsing size")?;
        if size.is_zero() {
            continue;
        }
        let time_ms = parse_iso_to_epoch_ms(&nt.time)?;
        trades.push(Trade {
            price,
            size,
            time_ms,
            is_buy: nt.side == "B",
        });
    }
    Ok(trades)
}

/// Parse ISO 8601 timestamp to epoch milliseconds.
/// Format: "2025-03-31T23:59:59.962208772"
fn parse_iso_to_epoch_ms(s: &str) -> Result<u64> {
    let dt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .context("parsing ISO timestamp")?;
    Ok(dt.and_utc().timestamp_millis() as u64)
}
