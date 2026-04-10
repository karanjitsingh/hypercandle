# hypercandle — Chat Summary

> Context for future AI agents working on this codebase.
> Generated: 2026-04-09

## What this project does

Rust CLI tool + library that builds OHLCV candle data from Hyperliquid's historical S3 fills. Downloads hourly LZ4-compressed NDJSON files from `s3://hl-mainnet-node-data` (requester-pays, ap-northeast-1), parses trade fills, and aggregates into candles at configurable intervals.

## Architecture

```
src/
  lib.rs      — DataSource enum, Market enum, S3 bucket const
  main.rs     — CLI with 3 subcommands: fetch, build, consolidate
  fetcher.rs  — S3 download with local file cache
  cache.rs    — Cache path management (data/s3/{bucket}/{prefix}/{date}/{hour}.lz4)
  parser.rs   — LZ4 decompress + JSON parse for 3 data formats
  candle.rs   — OHLCV aggregation, consolidation, CSV read/write
  spot.rs     — Spot coin resolution via Hyperliquid spotMeta API
```

## Key design decisions

### Three S3 data formats
Hyperliquid changed their fills format twice. The tool handles all three:
- `node_trades` (2025-03-22 → 2025-06-21): ISO timestamps, dedup by hash
- `node_fills` (2025-05-25 → 2025-07-27): `[address, fill]` per line, dedup by tid
- `node_fills_by_block` (2025-07-27 → present): blocks with events array

`DataSource::for_date()` returns a `Vec<DataSource>` — on transition dates (2025-05-25, 2025-07-27) it returns both sources so each hour tries both until one succeeds.

### Day-boundary spillover
S3 files are partitioned by block production time, not trade timestamp. Hour-0 of the next day may contain trades timestamped in the last seconds of the current day. The build command peeks at the next day's hour-0 file and filters all trades to exact day boundaries (00:00:00.000 → 23:59:59.999 UTC). This ensures each day is self-contained — running days separately produces identical results to running them together.

### Performance optimizations
- **Parallel processing**: rayon processes all 24 hourly files concurrently (read + decompress + parse)
- **JSON pre-filter**: `line.contains("\"BTC\"")` skips ~88% of lines before JSON deserialization
- **IgnoredAny**: serde skips address string allocation in fill tuples
- **Result**: 13.6s → 1.5s per day from cache (32-core machine)

### Consolidation
`consolidate` subcommand reads smaller-interval CSVs and re-buckets into larger intervals. Verified byte-identical to direct builds (e.g., 5m→1h matches building 1h directly from fills).

## Things to know

- **Missing 1m candles are normal**: BTC has ~2-10 minutes per day with zero trades (low activity periods). 30m and 1h candles are always complete.
- **First available date**: 2025-03-22 (partial day, hours 10-23 only)
- **Fills contain all coins**: each hourly file has fills for every coin. The pre-filter is critical for performance.
- **Deduplication**: fills appear twice (once per counterparty). Dedup by `tid` (or `hash` for node_trades).
- **Spot coins**: use `@{index}` internally (e.g., `@142` for UBTC/USDC). The tool resolves human-readable names like `BTCUSDC` via the spotMeta API.
- **`--benchmark` flag**: enables tracing instrumentation showing per-function timing (decompress, parse_fills, fetch_hourly).

## Data layout

```
data/
  s3/hl-mainnet-node-data/          # Raw S3 cache (gitignored)
    node_fills_by_block/hourly/YYYYMMDD/H.lz4
    node_fills/hourly/YYYYMMDD/H.lz4
    node_trades/hourly/YYYYMMDD/H.lz4
  candles/{market}/{coin}/{interval}/YYYYMMDD.csv    # Built from fills
  consolidated/{market}/{coin}/{interval}.csv           # Consolidated across selected date range
```

## Commit history highlights

1. Initial BTC-only implementation with single data source
2. Multi-coin support (any perp or spot coin)
3. Spot coin resolution via Hyperliquid API
4. Three S3 data format support with auto-selection
5. Day-boundary spillover handling
6. Transition date dual-source handling
7. Subcommands: fetch, build, consolidate
8. Performance: parallel processing + JSON pre-filter (7.5x speedup)
9. Tracing instrumentation behind --benchmark flag
10. Progress output with timing and fetch/cache counts

## Potential improvements not yet implemented

- `simd-json` + `memchr` (tested, marginal gain since LZ4 decompress dominates)
- Streaming LZ4 decompression (reduce memory from ~4.3 GB peak)
- Custom JSON scanner extracting only needed fields
- Concurrent S3 downloads in fetch command
- Date validation (warn if before 2025-03-22)
