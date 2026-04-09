use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;

use crate::DataSource;

// -- node_fills_by_block format --
#[derive(Debug, Deserialize)]
struct Block {
    events: Vec<(String, Fill)>,
}

// -- node_fills / node_fills_by_block shared fill struct --
#[derive(Debug, Deserialize)]
struct Fill {
    coin: String,
    px: String,
    sz: String,
    side: String,
    time: u64,
    tid: u64,
}

// -- node_trades format --
#[derive(Debug, Deserialize)]
struct NodeTrade {
    coin: String,
    px: String,
    sz: String,
    side: String,
    time: String, // ISO 8601
    hash: String,
}

/// A simplified trade extracted from a fill, filtered to the target coin.
#[derive(Debug, Clone)]
pub struct Trade {
    pub price: f64,
    pub size: f64,
    pub time_ms: u64,
    pub is_buy: bool,
}

/// Decompress LZ4 data and parse trades for the given coin.
/// Auto-selects parser based on the data source.
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

fn decompress(lz4_data: &[u8]) -> Result<String> {
    use std::io::Read;
    let mut decoder = lz4::Decoder::new(lz4_data).context("creating LZ4 decoder")?;
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).context("LZ4 decompression")?;
    String::from_utf8(buf).context("invalid UTF-8")
}

/// Current format: each line is a block with events array of [address, fill] pairs.
fn parse_fills_by_block(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let block: Block = serde_json::from_str(line).context("parsing block JSON")?;
        for (_addr, fill) in &block.events {
            if let Some(t) = extract_fill(fill, coin, &mut seen)? {
                trades.push(t);
            }
        }
    }
    Ok(trades)
}

/// Legacy format: each line is a flat [address, fill] pair.
fn parse_node_fills(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (_addr, fill): (String, Fill) =
            serde_json::from_str(line).context("parsing node_fill JSON")?;
        if let Some(t) = extract_fill(&fill, coin, &mut seen)? {
            trades.push(t);
        }
    }
    Ok(trades)
}

/// Shared fill extraction with tid dedup.
fn extract_fill(fill: &Fill, coin: &str, seen: &mut HashSet<u64>) -> Result<Option<Trade>> {
    if fill.coin != coin {
        return Ok(None);
    }
    if fill.tid == 0 || !seen.insert(fill.tid) {
        return Ok(None);
    }
    let price: f64 = fill.px.parse().context("parsing price")?;
    let size: f64 = fill.sz.parse().context("parsing size")?;
    if size == 0.0 {
        return Ok(None);
    }
    Ok(Some(Trade {
        price,
        size,
        time_ms: fill.time,
        is_buy: fill.side == "B",
    }))
}

/// Legacy node_trades format: each line is a trade object with ISO timestamp.
/// Already one trade per line (not duplicated per side), dedup by hash.
fn parse_node_trades(text: &str, coin: &str) -> Result<Vec<Trade>> {
    let mut trades = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let nt: NodeTrade = serde_json::from_str(line).context("parsing node_trade JSON")?;
        if nt.coin != coin || !seen.insert(nt.hash.clone()) {
            continue;
        }
        let price: f64 = nt.px.parse().context("parsing price")?;
        let size: f64 = nt.sz.parse().context("parsing size")?;
        if size == 0.0 {
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

fn parse_iso_to_epoch_ms(s: &str) -> Result<u64> {
    // Format: "2025-03-31T23:59:59.962208772"
    let dt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .context("parsing ISO timestamp")?;
    Ok(dt.and_utc().timestamp_millis() as u64)
}
