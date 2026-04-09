use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::Parser;
use hl_candles::{candle, fetcher, parser, spot, DataSource, Market};
use std::io::Write;
use std::path::Path;

const DATA_DIR: &str = "data";

#[derive(Parser)]
#[command(name = "hl-candles", about = "Build candle data from Hyperliquid S3 fills")]
struct Cli {
    /// Coin symbol (e.g. BTC, ETH, SOL for perp; BTCUSDC, ETHUSDC for spot)
    #[arg(long)]
    coin: String,

    /// Market type
    #[arg(long, value_enum, default_value = "perp")]
    market: Market,

    /// Start date (YYYYMMDD)
    #[arg(long)]
    start: String,

    /// End date inclusive (YYYYMMDD), defaults to start
    #[arg(long)]
    end: Option<String>,

    /// Candle interval: 1m, 5m, 15m, 1h, 4h, 1d
    #[arg(long, default_value = "1h")]
    interval: String,
}

/// Epoch ms for start of a UTC day.
fn day_start_ms(date: NaiveDate) -> u64 {
    date.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64
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

    let (coin, coin_label) = match cli.market {
        Market::Perp => {
            let c = cli.coin.to_uppercase();
            eprintln!("market: perp, coin: {c}");
            (c.clone(), c)
        }
        Market::Spot => {
            let label = cli.coin.to_uppercase();
            let resolved = spot::resolve_spot_coin(&cli.coin).await?;
            eprintln!("market: spot, pair: {label} -> {resolved}");
            (resolved, label)
        }
    };

    let market_str = match cli.market {
        Market::Perp => "perp",
        Market::Spot => "spot",
    };

    let data_dir = Path::new(DATA_DIR);
    let client = fetcher::create_client().await;

    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y%m%d").to_string();
        let sources = DataSource::for_date(&date_str);
        eprintln!("source: {:?} for {date_str}", sources);

        let day_start = day_start_ms(date);
        let day_end = day_start + 86_400_000; // exclusive

        // Fetch all 24 hours for this day, trying each source
        let mut day_trades = Vec::new();
        for hour in 0..24u8 {
            let mut found = false;
            for &source in &sources {
                match fetcher::fetch_hourly(&client, data_dir, &date_str, hour, source).await {
                    Ok(raw) => {
                        let trades = parser::parse_fills(&raw, &coin, source)?;
                        eprintln!("  {date_str}/{hour}: {} trades ({:?})", trades.len(), source);
                        day_trades.extend(trades);
                        found = true;
                        break;
                    }
                    Err(_) => continue,
                }
            }
            if !found {
                eprintln!("  {date_str}/{hour}: no data in any source");
            }
        }

        // Peek at next day's hour 0 to catch spillover trades belonging to this day
        let next_date = date + chrono::Duration::days(1);
        let next_date_str = next_date.format("%Y%m%d").to_string();
        let next_sources = DataSource::for_date(&next_date_str);
        for &source in &next_sources {
            if let Ok(raw) = fetcher::fetch_hourly(&client, data_dir, &next_date_str, 0, source).await {
                let spillover = parser::parse_fills(&raw, &coin, source)?;
                let count = spillover.iter().filter(|t| t.time_ms < day_end).count();
                if count > 0 {
                    eprintln!("  +{next_date_str}/0: {count} spillover trades");
                    day_trades.extend(spillover);
                }
                break;
            }
        }

        // Filter to only trades within this day's time range
        day_trades.retain(|t| t.time_ms >= day_start && t.time_ms < day_end);
        day_trades.sort_by_key(|t| t.time_ms);

        let candles = candle::aggregate(&day_trades, interval_ms);

        if !candles.is_empty() {
            let out_dir = data_dir
                .join("candles")
                .join(market_str)
                .join(&coin_label)
                .join(&cli.interval);
            std::fs::create_dir_all(&out_dir)?;
            let out_path = out_dir.join(format!("{date_str}.csv"));

            let mut f = std::fs::File::create(&out_path)?;
            writeln!(f, "open_time,close_time,open,high,low,close,volume,trades")?;
            for c in &candles {
                writeln!(
                    f,
                    "{},{},{},{},{},{},{},{}",
                    c.open_time, c.close_time, c.open, c.high, c.low, c.close, c.volume, c.trades
                )?;
            }
            eprintln!("  wrote {} candles -> {}", candles.len(), out_path.display());
        }

        date += chrono::Duration::days(1);
    }

    Ok(())
}
