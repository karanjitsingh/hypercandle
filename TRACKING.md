# TRACKING.md

## Project: hl-candles

Rust CLI tool/library to build BTC candle data from Hyperliquid's historical S3 fills.

## Progress

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | Research Hyperliquid docs | ✅ | S3 bucket `hl-mainnet-node-data` in `ap-northeast-1`, hourly LZ4 NDJSON files, perp=`BTC`, spot=`@142` |
| 2 | Scaffold Rust project | ✅ | Cargo project with aws-sdk-s3, lz4, clap, serde, chrono |
| 3 | S3 fetching with cache | ✅ | Requester-pays GET, local file cache at `{cache_dir}/{date}/{hour}.lz4` |
| 4 | Trade parsing & candle aggregation | ✅ | NDJSON parser, tid-based dedup, OHLCV aggregation at arbitrary intervals |
| 5 | CLI interface | ✅ | `--start`, `--end`, `--interval`, `--market`, `--cache-dir`, `--format` |
| 6 | Perp & spot support | ✅ | `--market perp` (coin `BTC`) and `--market spot` (coin `@142`) |
| 7 | Integration tests | ✅ | 11 tests using LZ4 fixture from real data, no S3 access needed |
| 8 | README & TRACKING | ✅ | This file + README.md |
| 9 | Final build verification | ✅ | `cargo build` and `cargo test` clean |

## Architecture

```
src/
  lib.rs      — public API, Market enum, constants
  fetcher.rs  — S3 client + fetch with cache
  cache.rs    — local file cache read/write
  parser.rs   — LZ4 decompress + NDJSON fill parsing + trade dedup
  candle.rs   — OHLCV candle aggregation + interval parsing
  main.rs     — CLI (clap)
tests/
  integration_test.rs   — 11 tests against fixture data
  fixtures/20250801/0.lz4  — small real-data extract
```

## Key decisions

- **LZ4 frame format**: The S3 files use LZ4 frame compression (not block), so we use the `lz4` crate (C binding) rather than `lz4_flex` which had overflow issues with some files.
- **Trade deduplication**: Each fill appears twice in the data (once per counterparty). We deduplicate by `tid` to avoid double-counting volume.
- **Spot BTC coin**: On Hyperliquid mainnet, spot BTC/USDC uses coin identifier `@142` (spot pair index). Perp uses plain `BTC`.
- **Requester-pays**: The S3 bucket requires `--request-payer requester`. Every download costs money, hence the mandatory local cache.
- **Bucket region**: `ap-northeast-1` (discovered via `HEAD` bucket — not documented).
