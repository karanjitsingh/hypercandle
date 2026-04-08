pub mod cache;
pub mod candle;
pub mod fetcher;
pub mod parser;

/// BTC perp coin identifier in Hyperliquid fills data
pub const BTC_PERP_COIN: &str = "BTC";
/// BTC spot coin identifier (spot index @142 on mainnet)
pub const BTC_SPOT_COIN: &str = "@142";

pub const S3_BUCKET: &str = "hl-mainnet-node-data";
pub const S3_PREFIX: &str = "node_fills_by_block/hourly";

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
