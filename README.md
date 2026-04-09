# hl-candles

Rust CLI tool and library to build BTC candle (OHLCV) data from [Hyperliquid's](https://hyperliquid.xyz) historical S3 fills data.

## How it works

Hyperliquid publishes raw trade fills to a public S3 bucket (`hl-mainnet-node-data`) as hourly LZ4-compressed NDJSON files. This tool downloads those files, extracts BTC trades, and aggregates them into candles at your chosen interval.

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
# 1-hour BTC perp candles for a single day
hl-candles --start 20250801 --interval 1h

# 5-minute candles for a date range, spot market, CSV output
hl-candles --start 20250801 --end 20250803 --interval 5m --market spot --format csv

# 1-day candles, custom cache directory
hl-candles --start 20250801 --end 20250831 --interval 1d --cache-dir /tmp/hl-cache
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | required | Start date (`YYYYMMDD`) |
| `--end` | same as start | End date inclusive (`YYYYMMDD`) |
| `--interval` | `1h` | Candle interval: `1m`, `5m`, `15m`, `1h`, `4h`, `1d` |
| `--market` | `perp` | `perp` (BTC-PERP) or `spot` (BTC/USDC spot, coin `@142`) |
| `--cache-dir` | `cache` | Local cache directory for downloaded S3 objects |
| `--format` | `json` | Output format: `json` or `csv` |

### Output

JSON output (array of candle objects):
```json
[
  {
    "open_time": 1754006400000,
    "close_time": 1754009999999,
    "open": 115724.0,
    "high": 115925.0,
    "low": 115200.0,
    "close": 115500.0,
    "volume": 1234.56,
    "trades": 15000
  }
]
```

CSV output:
```
open_time,close_time,open,high,low,close,volume,trades
1754006400000,1754009999999,115724.0,115925.0,115200.0,115500.0,1234.56,15000
```

## Library usage

```rust
use hl_candles::{fetcher, parser, candle, Market};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = fetcher::create_client().await;
    let cache_dir = std::path::Path::new("cache");

    let data = fetcher::fetch_hourly(&client, cache_dir, "20250801", 0).await?;
    let trades = parser::parse_fills(&data, Market::Perp.coin())?;
    let candles = candle::aggregate(&trades, candle::parse_interval("1h").unwrap());

    for c in &candles {
        println!("{}: O={} H={} L={} C={} V={:.4}", c.open_time, c.open, c.high, c.low, c.close, c.volume);
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
| `node_fills/` | Older fill format matching the API format | LZ4 NDJSON |
| `node_trades/` | Older trade format (does NOT match API format) | LZ4 NDJSON |
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
- The `node_fills_by_block/hourly/` prefix is the current/recommended format. `node_fills` and `node_trades` are legacy.
- **BTC contracts**: Perp uses `coin = "BTC"`, spot uses `coin = "@142"` (UBTC/USDC pair index on mainnet).
- **Docs**: https://hyperliquid.gitbook.io/hyperliquid-docs/historical-data

## Tests

```bash
cargo test
```

Tests use a small LZ4 fixture extracted from real data — no S3 access needed.

## Cost awareness

Each hourly file is 20-80 MB compressed. A full day is ~24 files. The S3 bucket is requester-pays, so you're charged for GET requests and data transfer. Files are cached locally after first download to avoid repeat costs.
