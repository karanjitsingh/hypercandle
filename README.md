# hl-candles

Rust CLI tool and library to build candle (OHLCV) data from [Hyperliquid's](https://hyperliquid.xyz) historical S3 fills data. Works for any coin — perp or spot.

## How it works

Hyperliquid publishes raw trade fills to a public S3 bucket (`hl-mainnet-node-data`) as hourly LZ4-compressed NDJSON files. This tool downloads those files, extracts trades for your chosen coin, and aggregates them into candles at your chosen interval.

Each downloaded file is cached locally so subsequent runs don't re-download (the bucket is requester-pays, so every fetch costs money).

## Prerequisites

- Rust toolchain (1.91+)
- AWS credentials configured (any method — env vars, `~/.aws/credentials`, SSO, etc.)
  - You pay S3 transfer costs (requester-pays bucket)

## Install

```bash
cargo install --path .
```

## Usage

```bash
# BTC perp 1-hour candles for a single day
hl-candles --coin BTC --start 20250801 --interval 1h

# ETH perp 5-minute candles for a date range, CSV output
hl-candles --coin ETH --start 20250801 --end 20250803 --interval 5m --format csv

# BTC spot (BTCUSDC) 1-day candles
hl-candles --coin BTCUSDC --market spot --start 20250801 --end 20250831 --interval 1d

# HYPE spot candles
hl-candles --coin HYPEUSDC --market spot --start 20260401 --interval 1h
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--coin` | required | Coin symbol. For perp: `BTC`, `ETH`, `SOL`, etc. For spot: pair like `BTCUSDC`, `ETHUSDC`, `HYPEUSDC` |
| `--market` | `perp` | `perp` or `spot` |
| `--start` | required | Start date (`YYYYMMDD`) |
| `--end` | same as start | End date inclusive (`YYYYMMDD`) |
| `--interval` | `1h` | Candle interval: `1m`, `5m`, `15m`, `1h`, `4h`, `1d` |
| `--cache-dir` | `cache` | Local cache directory for downloaded S3 objects |
| `--format` | `json` | Output format: `json` or `csv` |

### Output

JSON output (array of candle objects):
```json
[
  {
    "open_time": 1754006400000,
    "close_time": 1754009999999,
    "open": "115724.0",
    "high": "115925.0",
    "low": "115200.0",
    "close": "115500.0",
    "volume": "1234.56",
    "trades": 15000
  }
]
```

CSV output:
```
open_time,close_time,open,high,low,close,volume,trades
1754006400000,1754009999999,115724.0,115925.0,115200.0,115500.0,1234.56,15000
```

## Coin metadata

### Perp coins

Perp coins use their plain name as the identifier in the fills data. You can find all available perp coins via the Hyperliquid API:

```bash
curl -s -X POST https://api.hyperliquid.xyz/info \
  -H "Content-Type: application/json" \
  -d '{"type":"meta"}' | python3 -c "
import json, sys
data = json.load(sys.stdin)
for asset in data['universe']:
    print(asset['name'])
"
```

Common perp coins: `BTC`, `ETH`, `SOL`, `HYPE`, `DOGE`, `XRP`, `FARTCOIN`, etc.

### Spot coins

Spot coins use `@{index}` identifiers internally. The tool resolves human-readable pair names (e.g. `BTCUSDC`) to these indices automatically via the Hyperliquid `spotMeta` API.

To list all available spot pairs:

```bash
curl -s -X POST https://api.hyperliquid.xyz/info \
  -H "Content-Type: application/json" \
  -d '{"type":"spotMeta"}' | python3 -c "
import json, sys
data = json.load(sys.stdin)
tokens = {t['index']: t['name'] for t in data['tokens']}
for pair in data['universe']:
    base = tokens.get(pair['tokens'][0], '?')
    quote = tokens.get(pair['tokens'][1], '?')
    print(f\"@{pair['index']:>3}  {base}/{quote}\")
"
```

Key spot pairs:

| Pair | Internal ID | CLI usage |
|------|-------------|-----------|
| UBTC/USDC | `@142` | `--coin BTCUSDC --market spot` |
| UETH/USDC | `@151` | `--coin ETHUSDC --market spot` |
| USOL/USDC | `@156` | `--coin SOLUSDC --market spot` |
| HYPE/USDC | `@107` | `--coin HYPEUSDC --market spot` |
| PURR/USDC | `@0` | `--coin PURRUSDC --market spot` |

Note: On Hyperliquid, wrapped spot tokens use a `U` prefix (UBTC, UETH, USOL). The tool accepts both `BTCUSDC` and `UBTCUSDC` for convenience.

## Library usage

```rust
use hl_candles::{fetcher, parser, candle, DataSource};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = fetcher::create_client().await;
    let cache_dir = std::path::Path::new("cache");
    let source = DataSource::for_date("20250801");

    let data = fetcher::fetch_hourly(&client, cache_dir, "20250801", 0, source).await?;
    let trades = parser::parse_fills(&data, "BTC", source)?;
    let candles = candle::aggregate(&trades, candle::parse_interval("1h").unwrap());

    for c in &candles {
        println!("{}: O={} H={} L={} C={} V={}", c.open_time, c.open, c.high, c.low, c.close, c.volume);
    }
    Ok(())
}
```

## Data source

This tool uses fills from `hl-mainnet-node-data`. Below is a summary of all known Hyperliquid S3 buckets.

### `s3://hl-mainnet-node-data` (ap-northeast-1, requester-pays)

| Prefix | Content | Format |
|--------|---------|--------|
| `node_fills_by_block/hourly/{YYYYMMDD}/{hour}.lz4` | Trade fills grouped by block — **this is what we use**. Each line is a block with `events` array of `[address, fill]` pairs. Contains all coins (perp + spot). | LZ4 NDJSON |
| `node_fills/hourly/` | Older fill format matching the API format (2025-05 to 2025-07) | LZ4 NDJSON |
| `node_trades/hourly/` | Oldest trade format, different structure (2025-03 to 2025-06) | LZ4 NDJSON |
| `explorer_blocks/` | Historical explorer blocks (L1 block data) | LZ4 |
| `replica_cmds/` | Historical L1 transactions | LZ4 |

### `s3://hyperliquid-archive` (requester-pays, updated ~monthly)

| Prefix | Content | Format |
|--------|---------|--------|
| `market_data/{date}/{hour}/l2Book/{COIN}.lz4` | L2 order book snapshots per coin | LZ4 |
| `asset_ctxs/{date}.csv.lz4` | Asset context data (funding rates, open interest, etc.) | LZ4 CSV |

### Notes

- The archive bucket is updated infrequently (~monthly) with no guarantee of timeliness or completeness.
- Hyperliquid does **not** provide historical candle data via S3 — you must build candles from fills yourself, which is what this tool does.
- The `node_fills_by_block/hourly/` prefix is the current/recommended format. The tool auto-selects the right format based on date.
- **Docs**: https://hyperliquid.gitbook.io/hyperliquid-docs/historical-data

## Tests

```bash
cargo test
```

Tests use a small LZ4 fixture extracted from real data — no S3 access needed.

## Cost awareness

Each hourly file is 20-80 MB compressed. A full day is ~24 files. The S3 bucket is requester-pays, so you're charged for GET requests and data transfer. Files are cached locally after first download to avoid repeat costs.

Estimated cost to download all available data (~257 days): **~$16.50** (~145 GB at $0.114/GB transfer from ap-northeast-1).
