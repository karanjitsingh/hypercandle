pub mod cache;
pub mod candle;
pub mod fetcher;
pub mod parser;

/// BTC perp coin identifier in Hyperliquid fills data
pub const BTC_PERP_COIN: &str = "BTC";
/// BTC spot coin identifier (spot index @142 on mainnet)
pub const BTC_SPOT_COIN: &str = "@142";

pub const S3_BUCKET: &str = "hl-mainnet-node-data";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Market {
    Perp,
    Spot,
}

impl Market {
    pub fn coin(&self) -> &'static str {
        match self {
            Market::Perp => BTC_PERP_COIN,
            Market::Spot => BTC_SPOT_COIN,
        }
    }
}

/// Which S3 prefix / data format to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    /// `node_fills_by_block/hourly/` — current format, blocks with events array
    FillsByBlock,
    /// `node_fills/hourly/` — legacy, flat [address, fill] per line
    NodeFills,
    /// `node_trades/hourly/` — legacy, trade objects with ISO timestamps
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

    /// Pick the best data source for a given date.
    /// - node_trades:        20250322 – 20250621
    /// - node_fills:          20250525 – 20250727
    /// - node_fills_by_block: 20250727 – present
    /// Where ranges overlap, prefer the newer format.
    pub fn for_date(date: &str) -> Self {
        if date >= "20250727" {
            DataSource::FillsByBlock
        } else if date >= "20250525" {
            DataSource::NodeFills
        } else {
            DataSource::NodeTrades
        }
    }
}
