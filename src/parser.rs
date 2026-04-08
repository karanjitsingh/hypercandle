use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Block {
    pub events: Vec<(String, Fill)>,
}

#[derive(Debug, Deserialize)]
pub struct Fill {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub tid: u64,
}

/// A simplified trade extracted from a fill, filtered to the target coin.
#[derive(Debug, Clone)]
pub struct Trade {
    pub price: f64,
    pub size: f64,
    pub time_ms: u64,
    pub is_buy: bool,
}

/// Decompress LZ4 data and parse fills for the given coin.
pub fn parse_fills(lz4_data: &[u8], coin: &str) -> Result<Vec<Trade>> {
    use std::io::Read;
    let mut decoder = lz4::Decoder::new(lz4_data).context("creating LZ4 decoder")?;
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .context("LZ4 decompression")?;

    let text = std::str::from_utf8(&decompressed).context("invalid UTF-8")?;
    let mut trades = Vec::new();
    let mut seen_tids = std::collections::HashSet::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let block: Block = serde_json::from_str(line).context("parsing block JSON")?;
        for (_addr, fill) in &block.events {
            if fill.coin != coin {
                continue;
            }
            // Deduplicate by tid (each trade appears twice — once per side)
            if fill.tid == 0 || !seen_tids.insert(fill.tid) {
                continue;
            }
            let price: f64 = fill.px.parse().context("parsing price")?;
            let size: f64 = fill.sz.parse().context("parsing size")?;
            if size == 0.0 {
                continue;
            }
            trades.push(Trade {
                price,
                size,
                time_ms: fill.time,
                is_buy: fill.side == "B",
            });
        }
    }
    trades.sort_by_key(|t| t.time_ms);
    Ok(trades)
}
