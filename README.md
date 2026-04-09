# hypercandle

Rust CLI tool and library to build candle (OHLCV) data from [Hyperliquid's](https://hyperliquid.xyz) historical S3 fills data.

## How it works

Hyperliquid publishes raw trade fills to a public S3 bucket (`hl-mainnet-node-data`) as hourly LZ4-compressed NDJSON files. This tool downloads those files, extracts trades for your chosen coin, and aggregates them into candles at your chosen interval.

Each downloaded file is cached locally so subsequent runs don't re-download (the bucket is requester-pays, so every fetch costs money).

## Prerequisites

- AWS credentials configured (any method — env vars, `~/.aws/credentials`, SSO, etc.)
  - You pay S3 transfer costs (requester-pays bucket)

## Usage

Three subcommands: `fetch`, `build`, and `consolidate`.

### `fetch` — Download raw S3 data

Download hourly LZ4 files to local cache without building candles.

```bash
# Fetch a single day
hypercandle fetch --start 20250801

# Fetch a date range
hypercandle fetch --start 20250801 --end 20250831
```

### `build` — Build candles from S3 fills

```bash
# BTC perp 1-minute candles for a single day
hypercandle build --coin BTC --start 20250801 --interval 1m

# ETH perp 1-hour candles for a date range
hypercandle build --coin ETH --start 20250801 --end 20250803 --interval 1h

# BTC spot (BTCUSDC) 1-minute candles
hypercandle build --coin BTCUSDC --market spot --start 20250801 --interval 1m

# HYPE spot candles
hypercandle build --coin HYPEUSDC --market spot --start 20260401 --interval 1h
```

### `consolidate` — Merge smaller candles into larger intervals

```bash
# Consolidate 1m candles into 30m
hypercandle consolidate --coin BTC --start 20250801 --end 20250831 --from 1m --to 30m

# Consolidate 1m candles into 1h
hypercandle consolidate --coin BTC --start 20250801 --end 20250831 --from 1m --to 1h
```

### Options

#### Global

| Flag | Description |
|------|-------------|
| `--benchmark` | Enable tracing instrumentation for performance profiling |

#### `fetch`

| Flag | Default | Description |
|------|---------|-------------|
| `--start` | required | Start date (`YYYYMMDD`) |
| `--end` | same as start | End date inclusive (`YYYYMMDD`) |

#### `build`

| Flag | Default | Description |
|------|---------|-------------|
| `--coin` | required | Coin symbol. Perp: `BTC`, `ETH`, `SOL`. Spot: `BTCUSDC`, `ETHUSDC`, `HYPEUSDC` |
| `--market` | `perp` | `perp` or `spot` |
| `--start` | required | Start date (`YYYYMMDD`) |
| `--end` | same as start | End date inclusive (`YYYYMMDD`) |
| `--interval` | `1h` | Candle interval: `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, `1d` |

#### `consolidate`

| Flag | Default | Description |
|------|---------|-------------|
| `--coin` | required | Coin symbol (same as build) |
| `--market` | `perp` | `perp` or `spot` |
| `--start` | required | Start date (`YYYYMMDD`) |
| `--end` | same as start | End date inclusive (`YYYYMMDD`) |
| `--from` | required | Source interval to read from (e.g. `1m`) |
| `--to` | required | Target interval to consolidate into (e.g. `30m`, `1h`, `1d`) |

### Output

Candles are written as CSV files to `data/candles/{market}/{coin}/{interval}/{date}.csv` (build) or `data/consolidated/{market}/{coin}/{interval}/{date}.csv` (consolidate).

```
open_time,close_time,open,high,low,close,volume,trades
1754006400000,1754009999999,115724.0,115925.0,115200.0,115500.0,1234.56,15000
```

Build progress is printed per day:
```
market: perp, coin: BTC
[1/3] 20250801 24 candles, 564339 trades (1.5s, total 1.5s, 0 fetched/24 cached)
[2/3] 20250802 24 candles, 269790 trades (1.0s, total 2.5s, 0 fetched/24 cached)
[3/3] 20250803 24 candles, 220477 trades (0.7s, total 3.2s, 0 fetched/24 cached)
done
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
use hypercandle::{fetcher, parser, candle, DataSource};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = fetcher::create_client().await;
    let data_dir = std::path::Path::new("data");
    let sources = DataSource::for_date("20250801");

    let data = fetcher::fetch_hourly(&client, data_dir, "20250801", 0, sources[0]).await?;
    let trades = parser::parse_fills(&data, "BTC", sources[0])?;
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

### Source selection and boundary handling

The tool auto-selects the correct data source based on date:

| Date range | Source | Notes |
|---|---|---|
| 2025-03-22 → 2025-05-24 | `node_trades` | Oldest format, ISO timestamps, dedup by hash |
| **2025-05-25** | `node_trades` + `node_fills` | **Transition day**: `node_trades` has hours 0-23, `node_fills` starts at hour 14 |
| 2025-05-26 → 2025-07-26 | `node_fills` | Flat `[addr, fill]` per line, dedup by tid |
| **2025-07-27** | `node_fills` + `node_fills_by_block` | **Transition day**: `node_fills` has hours 0-8, `node_fills_by_block` starts at hour 8 |
| 2025-07-28 → present | `node_fills_by_block` | Current format, blocks with events array |

On transition dates, both sources are tried per hour to ensure full 24-hour coverage.

### Day-boundary spillover

S3 files are partitioned by block production time, not trade timestamp. The hour-0 file for a given date may contain a few trades timestamped in the last seconds of the previous day. The tool handles this by:

1. Peeking at the next day's hour-0 file to catch spillover trades
2. Filtering all trades to the exact day boundary (00:00:00.000 → 23:59:59.999 UTC)

This ensures each day produces exactly 24 hourly candles with no gaps or overlaps, regardless of whether days are processed together or separately.

### `s3://hyperliquid-archive` (requester-pays, updated ~monthly)

| Prefix | Content | Format |
|--------|---------|--------|
| `market_data/{date}/{hour}/l2Book/{COIN}.lz4` | L2 order book snapshots per coin | LZ4 |
| `asset_ctxs/{date}.csv.lz4` | Asset context data (funding rates, open interest, etc.) | LZ4 CSV |

### Notes

- The archive bucket is updated infrequently (~monthly) with no guarantee of timeliness or completeness.
- Hyperliquid does **not** provide historical candle data via S3 — you must build candles from fills yourself, which is what this tool does.
- **Docs**: https://hyperliquid.gitbook.io/hyperliquid-docs/historical-data

## Performance

Processing is parallelized across CPU cores using rayon. A JSON pre-filter skips ~88% of lines that don't contain the target coin before deserialization.

| Metric | Value |
|---|---|
| Per day (from cache) | ~1.5s |
| Per day (downloading) | ~20s (depends on network) |
| Full history (384 days, BTC, from cache) | ~10 min |

Use `--benchmark` to enable tracing instrumentation for per-function timing.

## Tests

```bash
cargo test
```

Tests use a small LZ4 fixture extracted from real data — no S3 access needed.

## Cost awareness

Each hourly file is 20-80 MB compressed. A full day is ~24 files. The S3 bucket is requester-pays, so you're charged for GET requests and data transfer. Files are cached locally after first download to avoid repeat costs.

Estimated cost to download all available data (~384 days): **~$25** (~220 GB at $0.114/GB transfer from ap-northeast-1).

---

> Generated by AI. Use at your own risk.