use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::Parser;
use hl_candles::{candle, fetcher, parser, spot, DataSource, Market};
use std::io::Write;
use std::path::Path;

/// All data lives under this directory:
/// - `data/s3/...`      — cached raw S3 downloads (LZ4 compressed)
/// - `data/candles/...`  — generated candle CSVs
const DATA_DIR: &str = "data";

#[derive(Parser)]
#[command(name = "hl-candles", about = "Build candle data from Hyperliquid S3 fills")]
struct Cli {
    /// Coin symbol. For perp: BTC, ETH, SOL, etc.
    /// For spot: pair name like BTCUSDC, ETHUSDC, HYPEUSDC.
    #[arg(long)]
    coin: String,

    /// Market type. Perp uses coin name directly. Spot resolves the pair
    /// name to an @index via the Hyperliquid spotMeta API.
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

/// Epoch ms for 00:00:00.000 UTC of the given date.
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

    // Resolve the coin identifier.
    // Perp: use as-is (e.g. "BTC"). Spot: resolve pair name to @index.
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

    // Process one day at a time. Each day produces one CSV file.
    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y%m%d").to_string();

        // Get the data source(s) for this date. On transition dates (20250525,
        // 20250727) multiple sources are returned — we try each per hour and
        // use the first that has data.
        let sources = DataSource::for_date(&date_str);
        eprintln!("source: {:?} for {date_str}", sources);

        // Day boundaries in epoch ms for filtering trades.
        let day_start = day_start_ms(date);
        let day_end = day_start + 86_400_000; // exclusive

        // Fetch all 24 hours, trying each source until one succeeds.
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

        // S3 files are partitioned by block production time, not trade timestamp.
        // The hour-0 file for the next day may contain trades timestamped in the
        // last seconds of the current day. Peek at it to avoid losing those trades.
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

        // Filter to exactly this day's time range [00:00:00.000, 23:59:59.999].
        // This ensures each day produces exactly 24 hourly candles with no
        // overlap between consecutive days, regardless of run order.
        day_trades.retain(|t| t.time_ms >= day_start && t.time_ms < day_end);
        day_trades.sort_by_key(|t| t.time_ms);

        let candles = candle::aggregate(&day_trades, interval_ms);

        // Write candles to data/candles/{market}/{coin}/{interval}/{date}.csv
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
