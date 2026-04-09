use crate::parser::Trade;
use rust_decimal::Decimal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Candle {
    pub open_time: u64,
    pub close_time: u64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trades: u64,
}

/// Aggregate trades into candles of the given interval (in milliseconds).
pub fn aggregate(trades: &[Trade], interval_ms: u64) -> Vec<Candle> {
    if trades.is_empty() {
        return Vec::new();
    }

    let mut candles: Vec<Candle> = Vec::new();
    let mut current: Option<Candle> = None;

    for trade in trades {
        let bucket_start = (trade.time_ms / interval_ms) * interval_ms;
        let bucket_end = bucket_start + interval_ms - 1;

        match current.as_mut() {
            Some(c) if c.open_time == bucket_start => {
                if trade.price > c.high {
                    c.high = trade.price;
                }
                if trade.price < c.low {
                    c.low = trade.price;
                }
                c.close = trade.price;
                c.volume += trade.size;
                c.trades += 1;
            }
            _ => {
                if let Some(c) = current.take() {
                    candles.push(c);
                }
                current = Some(Candle {
                    open_time: bucket_start,
                    close_time: bucket_end,
                    open: trade.price,
                    high: trade.price,
                    low: trade.price,
                    close: trade.price,
                    volume: trade.size,
                    trades: 1,
                });
            }
        }
    }
    if let Some(c) = current {
        candles.push(c);
    }
    candles
}

/// Parse interval string like "1m", "5m", "1h", "1d" into milliseconds.
pub fn parse_interval(s: &str) -> Option<u64> {
    let (num, unit) = s.split_at(s.len() - 1);
    let n: u64 = num.parse().ok()?;
    let ms = match unit {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => return None,
    };
    Some(ms)
}
