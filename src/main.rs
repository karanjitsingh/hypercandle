use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::Parser;
use hl_candles::{candle, fetcher, parser, Market};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hl-candles", about = "Build BTC candle data from Hyperliquid S3 fills")]
struct Cli {
    /// Start date (YYYYMMDD)
    #[arg(long)]
    start: String,

    /// End date inclusive (YYYYMMDD), defaults to start
    #[arg(long)]
    end: Option<String>,

    /// Candle interval: 1m, 5m, 15m, 1h, 4h, 1d
    #[arg(long, default_value = "1h")]
    interval: String,

    /// Market type
    #[arg(long, value_enum, default_value = "perp")]
    market: Market,

    /// Cache directory for downloaded S3 objects
    #[arg(long, default_value = "cache")]
    cache_dir: PathBuf,

    /// Output format: json or csv
    #[arg(long, default_value = "json")]
    format: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let interval_ms = candle::parse_interval(&cli.interval)
        .ok_or_else(|| anyhow::anyhow!("invalid interval: {}", cli.interval))?;

    let start = NaiveDate::parse_from_str(&cli.start, "%Y%m%d")?;
    let end = match &cli.end {
        Some(e) => NaiveDate::parse_from_str(e, "%Y%m%d")?,
        None => start,
    };
    if end < start {
        bail!("end date must be >= start date");
    }

    let client = fetcher::create_client().await;
    let coin = cli.market.coin();
    let mut all_trades = Vec::new();

    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y%m%d").to_string();
        for hour in 0..24u8 {
            match fetcher::fetch_hourly(&client, &cli.cache_dir, &date_str, hour).await {
                Ok(data) => {
                    let trades = parser::parse_fills(&data, coin)?;
                    eprintln!("  {date_str}/{hour}: {} trades", trades.len());
                    all_trades.extend(trades);
                }
                Err(e) => {
                    eprintln!("  {date_str}/{hour}: skipped ({e:#})");
                }
            }
        }
        date += chrono::Duration::days(1);
    }

    all_trades.sort_by_key(|t| t.time_ms);
    let candles = candle::aggregate(&all_trades, interval_ms);
    eprintln!("\n{} candles generated", candles.len());

    match cli.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&candles)?),
        "csv" => {
            println!("open_time,close_time,open,high,low,close,volume,trades");
            for c in &candles {
                println!(
                    "{},{},{},{},{},{},{},{}",
                    c.open_time, c.close_time, c.open, c.high, c.low, c.close, c.volume, c.trades
                );
            }
        }
        _ => bail!("unknown format: {}", cli.format),
    }

    Ok(())
}
