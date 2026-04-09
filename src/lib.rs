//! hl-candles: Build OHLCV candle data from Hyperliquid's historical S3 fills.
//!
//! Hyperliquid publishes trade fills to `s3://hl-mainnet-node-data` in three
//! formats across different date ranges. This library handles all three and
//! auto-selects the correct source based on date.

pub mod cache;
pub mod candle;
pub mod fetcher;
pub mod parser;
pub mod spot;

pub const S3_BUCKET: &str = "hl-mainnet-node-data";

/// Perp or spot market. Perp coins use plain names (BTC, ETH).
/// Spot coins use `@{index}` identifiers resolved via the Hyperliquid API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Market {
    Perp,
    Spot,
}

/// Which S3 prefix and data format to use for a given date.
///
/// Hyperliquid has published fills in three different formats over time:
///
/// | Source          | S3 prefix                        | Date range              | Format                                    |
/// |-----------------|----------------------------------|-------------------------|-------------------------------------------|
/// | `NodeTrades`    | `node_trades/hourly/`            | 2025-03-22 → 2025-06-21 | One trade object per line, ISO timestamps  |
/// | `NodeFills`     | `node_fills/hourly/`             | 2025-05-25 → 2025-07-27 | One `[address, fill]` pair per line        |
/// | `FillsByBlock`  | `node_fills_by_block/hourly/`    | 2025-07-27 → present    | Blocks with `events` array of fills        |
///
/// The ranges overlap at transition dates:
/// - **2025-05-25**: `NodeTrades` has hours 0-23, `NodeFills` starts at hour 14
/// - **2025-07-27**: `NodeFills` has hours 0-8, `FillsByBlock` starts at hour 8
///
/// On these transition dates, both sources are tried per hour to ensure
/// full 24-hour coverage with no missed trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    /// `node_fills_by_block/hourly/` — current format. Each NDJSON line is a
    /// block containing an `events` array of `[address, fill]` pairs.
    FillsByBlock,
    /// `node_fills/hourly/` — legacy format. Each NDJSON line is a single
    /// `[address, fill]` pair (same fill schema as FillsByBlock).
    NodeFills,
    /// `node_trades/hourly/` — oldest format. Each NDJSON line is a trade
    /// object with ISO timestamps and a `side_info` array. Does NOT match
    /// the API format.
    NodeTrades,
}

impl DataSource {
    pub fn s3_prefix(&self) -> &'static str {
        match self {
            DataSource::FillsByBlock => "node_fills_by_block/hourly",
            DataSource::NodeFills => "node_fills/hourly",
            DataSource::NodeTrades => "node_trades/hourly",
        }
    }

    /// Returns the data source(s) to try for a given date (YYYYMMDD).
    ///
    /// On transition dates where one source ends and another begins mid-day,
    /// returns both sources. The caller should try each source per hour and
    /// use the first one that succeeds.
    pub fn for_date(date: &str) -> Vec<Self> {
        match date {
            // Transition days: NodeTrades ends, NodeFills starts at hour 14
            "20250525" => vec![DataSource::NodeTrades, DataSource::NodeFills],
            // Transition days: NodeFills ends at hour 8, FillsByBlock starts
            "20250727" => vec![DataSource::NodeFills, DataSource::FillsByBlock],
            d if d < "20250525" => vec![DataSource::NodeTrades],
            d if d < "20250727" => vec![DataSource::NodeFills],
            _ => vec![DataSource::FillsByBlock],
        }
    }
}
