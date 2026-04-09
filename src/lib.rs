pub mod cache;
pub mod candle;
pub mod fetcher;
pub mod parser;
pub mod spot;

pub const S3_BUCKET: &str = "hl-mainnet-node-data";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Market {
    Perp,
    Spot,
}

/// Which S3 prefix / data format to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    FillsByBlock,
    NodeFills,
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
