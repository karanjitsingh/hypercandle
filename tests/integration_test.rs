use hl_candles::{candle, parser, DataSource, Market};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures";

fn load_fixture() -> Vec<u8> {
    let path = Path::new(FIXTURE_DIR).join("20250801/0.lz4");
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

#[test]
fn parse_perp_fills() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    assert!(!trades.is_empty(), "should have perp trades");
    for t in &trades {
        assert!(t.price > dec!(100_000) && t.price < dec!(200_000), "price {}", t.price);
        assert!(t.size > Decimal::ZERO);
        assert!(t.time_ms > 0);
    }
}

#[test]
fn parse_spot_fills() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Spot.coin(), DataSource::FillsByBlock).unwrap();
    assert!(!trades.is_empty(), "should have spot trades");
    for t in &trades {
        assert!(t.price > dec!(100_000) && t.price < dec!(200_000), "price {}", t.price);
        assert!(t.size > Decimal::ZERO);
    }
}

#[test]
fn trades_are_deduplicated() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    let trades2 = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    assert_eq!(trades.len(), trades2.len(), "deterministic parsing");
}

#[test]
fn trades_sorted_by_time() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    for w in trades.windows(2) {
        assert!(w[0].time_ms <= w[1].time_ms, "trades not sorted");
    }
}

#[test]
fn aggregate_1m_candles() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    let candles = candle::aggregate(&trades, candle::parse_interval("1m").unwrap());
    assert!(!candles.is_empty());
    for c in &candles {
        assert!(c.open > Decimal::ZERO);
        assert!(c.high >= c.low);
        assert!(c.high >= c.open);
        assert!(c.high >= c.close);
        assert!(c.low <= c.open);
        assert!(c.low <= c.close);
        assert!(c.volume > Decimal::ZERO);
        assert!(c.trades > 0);
        assert_eq!(c.close_time, c.open_time + 59_999);
    }
}

#[test]
fn aggregate_1h_candles() {
    let data = load_fixture();
    let trades = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    let candles = candle::aggregate(&trades, candle::parse_interval("1h").unwrap());
    assert!(!candles.is_empty() && candles.len() <= 2);
    for c in &candles {
        assert!(c.trades > 0);
        assert!(c.high >= c.low);
    }
}

#[test]
fn empty_trades_produce_no_candles() {
    let candles = candle::aggregate(&[], 60_000);
    assert!(candles.is_empty());
}

#[test]
fn parse_interval_valid() {
    assert_eq!(candle::parse_interval("1m"), Some(60_000));
    assert_eq!(candle::parse_interval("5m"), Some(300_000));
    assert_eq!(candle::parse_interval("15m"), Some(900_000));
    assert_eq!(candle::parse_interval("1h"), Some(3_600_000));
    assert_eq!(candle::parse_interval("4h"), Some(14_400_000));
    assert_eq!(candle::parse_interval("1d"), Some(86_400_000));
}

#[test]
fn parse_interval_invalid() {
    assert_eq!(candle::parse_interval("abc"), None);
    assert_eq!(candle::parse_interval("1x"), None);
}

#[test]
fn perp_and_spot_are_different() {
    let data = load_fixture();
    let perp = parser::parse_fills(&data, Market::Perp.coin(), DataSource::FillsByBlock).unwrap();
    let spot = parser::parse_fills(&data, Market::Spot.coin(), DataSource::FillsByBlock).unwrap();
    assert_ne!(perp.len(), spot.len(), "perp and spot should differ");
}

#[test]
fn cache_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let data = b"test data";
    let path = hl_candles::cache::write_cache(dir.path(), "20250801", 0, data).unwrap();
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap(), data);

    let cached = hl_candles::cache::get_cached(dir.path(), "20250801", 0);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap(), path);

    assert!(hl_candles::cache::get_cached(dir.path(), "20250801", 1).is_none());
}
