//! Spot coin resolution via the Hyperliquid API.
//!
//! Spot coins use `@{index}` identifiers in the fills data (e.g. `@142`
//! for UBTC/USDC). This module resolves human-readable pair names like
//! "BTCUSDC" to these indices by querying the `spotMeta` API endpoint.
//!
//! Common aliases are supported: BTC→UBTC, ETH→UETH, SOL→USOL, since
//! Hyperliquid uses wrapped token names (UBTC, UETH, USOL) for spot.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct SpotMeta {
    universe: Vec<SpotPair>,
    tokens: Vec<Token>,
}

#[derive(Debug, Deserialize)]
struct SpotPair {
    index: u32,
    tokens: [u32; 2],
}

#[derive(Debug, Deserialize)]
struct Token {
    index: u32,
    name: String,
}

/// Resolve a human-readable spot pair to its `@{index}` coin identifier.
///
/// Accepts formats like "BTCUSDC", "BTC/USDC", "UBTCUSDC".
/// Automatically maps BTC→UBTC, ETH→UETH, SOL→USOL.
pub async fn resolve_spot_coin(pair: &str) -> Result<String> {
    let meta = fetch_spot_meta().await?;

    let token_names: HashMap<u32, &str> = meta.tokens.iter().map(|t| (t.index, t.name.as_str())).collect();

    // Normalize separators: BTC/USDC, BTC-USDC, BTC_USDC → BTCUSDC
    let normalized = pair.to_uppercase().replace(['/', '-', '_'], "");

    // Wrapped token aliases for spot markets
    let aliases: &[(&str, &str)] = &[
        ("BTC", "UBTC"), ("ETH", "UETH"), ("SOL", "USOL"),
    ];

    for sp in &meta.universe {
        let base = token_names.get(&sp.tokens[0]).unwrap_or(&"?");
        let quote = token_names.get(&sp.tokens[1]).unwrap_or(&"?");
        let candidate = format!("{base}{quote}");

        // Direct match (e.g. HYPEUSDC, UBTCUSDC)
        if candidate == normalized {
            return Ok(format!("@{}", sp.index));
        }
        // Alias match (e.g. BTCUSDC → UBTCUSDC)
        for (from, to) in aliases {
            let aliased = normalized.replacen(from, to, 1);
            if candidate == aliased {
                return Ok(format!("@{}", sp.index));
            }
        }
    }

    bail!("unknown spot pair: {pair}. Use format like BTCUSDC, ETHUSDC, HYPEUSDC")
}

async fn fetch_spot_meta() -> Result<SpotMeta> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.hyperliquid.xyz/info")
        .json(&serde_json::json!({"type": "spotMeta"}))
        .send()
        .await
        .context("fetching spotMeta")?;
    resp.json().await.context("parsing spotMeta")
}

/// List all available spot pairs as (human_name, @index) tuples.
pub async fn list_spot_pairs() -> Result<Vec<(String, String)>> {
    let meta = fetch_spot_meta().await?;
    let token_names: HashMap<u32, &str> = meta.tokens.iter().map(|t| (t.index, t.name.as_str())).collect();

    let mut pairs = Vec::new();
    for sp in &meta.universe {
        let base = token_names.get(&sp.tokens[0]).unwrap_or(&"?");
        let quote = token_names.get(&sp.tokens[1]).unwrap_or(&"?");
        pairs.push((format!("{base}{quote}"), format!("@{}", sp.index)));
    }
    Ok(pairs)
}
