use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use hl_candles::{candle, fetcher, parser, spot, DataSource, Market};
use rayon::prelude::*;
use std::io::Write;
use std::path::Path;

/// All data lives under this directory:
/// - `data/s3/...`           — cached raw S3 downloads (LZ4 compressed)
/// - `data/candles/...`      — candles built from raw fills
/// - `data/consolidated/...` — candles consolidated from smaller intervals
const DATA_DIR: &str = "data";

#[derive(Parser)]
#[command(name = "hl-candles", about = "Build candle data from Hyperliquid S3 fills")]
struct Cli {
    /// Enable tracing instrumentation for performance profiling
    #[arg(long, global = true)]
    benchmark: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch raw S3 fills data to local cache (no candle building)
    Fetch {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: Option<String>,
    },
    /// Build candles from S3 fills data
    Build {
        #[arg(long)]
        coin: String,
        #[arg(long, value_enum, default_value = "perp")]
        market: Market,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: Option<String>,
        /// Candle interval: 1m, 5m, 15m, 1h, 4h, 1d
        #[arg(long, default_value = "1h")]
        interval: String,
    },
    /// Consolidate smaller-interval candles into a larger interval
    Consolidate {
        #[arg(long)]
        coin: String,
        #[arg(long, value_enum, default_value = "perp")]
        market: Market,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: Option<String>,
        /// Source interval to read from (e.g. 5m)
        #[arg(long)]
        from: String,
        /// Target interval to consolidate into (e.g. 1h, 4h, 1d)
        #[arg(long)]
        to: String,
    },
}

fn day_start_ms(date: NaiveDate) -> u64 {
    date.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64
}

fn write_candles(path: &Path, candles: &[candle::Candle]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "open_time,close_time,open,high,low,close,volume,trades")?;
    for c in candles {
        writeln!(
            f, "{},{},{},{},{},{},{},{}",
            c.open_time, c.close_time, c.open, c.high, c.low, c.close, c.volume, c.trades
        )?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.benchmark {
        use tracing_subscriber::fmt::format::FmtSpan;
        tracing_subscriber::fmt()
            .with_span_events(FmtSpan::CLOSE)
            .with_target(false)
            .init();
    }

    match cli.command {
        Command::Fetch { start, end } => {
            cmd_fetch(start, end).await
        }
        Command::Build { coin, market, start, end, interval } => {
            cmd_build(coin, market, start, end, interval).await
        }
        Command::Consolidate { coin, market, start, end, from, to } => {
            cmd_consolidate(coin, market, start, end, from, to)
        }
    }
}

async fn cmd_fetch(start: String, end: Option<String>) -> Result<()> {
    let start = NaiveDate::parse_from_str(&start, "%Y%m%d")?;
    let end = match &end {
        Some(e) => NaiveDate::parse_from_str(e, "%Y%m%d")?,
        None => start,
    };
    if end < start { bail!("end date must be >= start date"); }

    let data_dir = Path::new(DATA_DIR);
    let client = fetcher::create_client().await;
    let mut downloaded = 0u32;
    let mut cached = 0u32;
    let mut failed = 0u32;

    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y%m%d").to_string();
        let sources = DataSource::for_date(&date_str);

        for hour in 0..24u8 {
            let mut found = false;
            for &source in &sources {
                if hl_candles::cache::get_cached(data_dir, &date_str, hour, source).is_some() {
                    cached += 1;
                    found = true;
                    break;
                }
                match fetcher::fetch_hourly(&client, data_dir, &date_str, hour, source).await {
                    Ok(data) => {
                        println!("  {date_str}/{hour}: {} bytes ({:?})", data.len(), source);
                        downloaded += 1;
                        found = true;
                        break;
                    }
                    Err(_) => continue,
                }
            }
            if !found { failed += 1; }
        }

        date += chrono::Duration::days(1);
    }

    println!("\nfetch complete: {downloaded} downloaded, {cached} cached, {failed} failed");
    Ok(())
}

async fn cmd_build(
    coin_arg: String, market: Market, start: String, end: Option<String>, interval: String,
) -> Result<()> {
    let interval_ms = candle::parse_interval(&interval)
        .ok_or_else(|| anyhow::anyhow!("invalid interval: {interval}"))?;

    let start = NaiveDate::parse_from_str(&start, "%Y%m%d")?;
    let end = match &end {
        Some(e) => NaiveDate::parse_from_str(e, "%Y%m%d")?,
        None => start,
    };
    if end < start { bail!("end date must be >= start date"); }

    let (coin, coin_label) = match market {
        Market::Perp => {
            let c = coin_arg.to_uppercase();
            println!("market: perp, coin: {c}");
            (c.clone(), c)
        }
        Market::Spot => {
            let label = coin_arg.to_uppercase();
            let resolved = spot::resolve_spot_coin(&coin_arg).await?;
            println!("market: spot, pair: {label} -> {resolved}");
            (resolved, label)
        }
    };

    let market_str = match market { Market::Perp => "perp", Market::Spot => "spot" };
    let data_dir = Path::new(DATA_DIR);
    let client = fetcher::create_client().await;

    let total_days = (end - start).num_days() + 1;
    let mut day_num = 0i64;

    let mut date = start;
    while date <= end {
        day_num += 1;
        let t0 = std::time::Instant::now();
        let date_str = date.format("%Y%m%d").to_string();
        let sources = DataSource::for_date(&date_str);

        let day_start = day_start_ms(date);
        let day_end = day_start + 86_400_000;

        // Ensure all hours are cached (sequential S3 fetches if needed)
        for hour in 0..24u8 {
            for &source in &sources {
                if hl_candles::cache::get_cached(data_dir, &date_str, hour, source).is_some() {
                    break;
                }
                if fetcher::fetch_hourly(&client, data_dir, &date_str, hour, source).await.is_ok() {
                    break;
                }
            }
        }

        // Parallel decompress + parse all 24 hours using rayon
        let hours: Vec<u8> = (0..24).collect();
        let coin_ref = &coin;
        let sources_ref = &sources;
        let hour_results: Vec<_> = hours.par_iter().filter_map(|&hour| {
            for &source in sources_ref {
                if let Some(path) = hl_candles::cache::get_cached(data_dir, &date_str, hour, source) {
                    if let Ok(raw) = std::fs::read(&path) {
                        if let Ok(trades) = parser::parse_fills(&raw, coin_ref, source) {
                            return Some(trades);
                        }
                    }
                }
            }
            None
        }).collect();

        let mut day_trades: Vec<_> = hour_results.into_iter().flatten().collect();

        // Peek at next day's hour 0 for spillover trades
        let next_date = date + chrono::Duration::days(1);
        let next_date_str = next_date.format("%Y%m%d").to_string();
        let next_sources = DataSource::for_date(&next_date_str);
        for &source in &next_sources {
            if let Ok(raw) = fetcher::fetch_hourly(&client, data_dir, &next_date_str, 0, source).await {
                let spillover = parser::parse_fills(&raw, &coin, source)?;
                let count = spillover.iter().filter(|t| t.time_ms < day_end).count();
                if count > 0 {
                    day_trades.extend(spillover);
                }
                break;
            }
        }

        day_trades.retain(|t| t.time_ms >= day_start && t.time_ms < day_end);
        day_trades.sort_by_key(|t| t.time_ms);

        let candles = candle::aggregate(&day_trades, interval_ms);
        if !candles.is_empty() {
            let out_path = data_dir
                .join("candles").join(market_str).join(&coin_label)
                .join(&interval).join(format!("{date_str}.csv"));
            write_candles(&out_path, &candles)?;
            let elapsed = t0.elapsed();
            println!("[{day_num}/{total_days}] {date_str} {} candles, {} trades ({:.1}s)", candles.len(), day_trades.len(), elapsed.as_secs_f64());
        } else {
            let elapsed = t0.elapsed();
            println!("[{day_num}/{total_days}] {date_str} no trades ({:.1}s)", elapsed.as_secs_f64());
        }

        date += chrono::Duration::days(1);
    }
    Ok(())
}

fn cmd_consolidate(
    coin_arg: String, market: Market, start: String, end: Option<String>,
    from: String, to: String,
) -> Result<()> {
    let from_ms = candle::parse_interval(&from)
        .ok_or_else(|| anyhow::anyhow!("invalid source interval: {from}"))?;
    let to_ms = candle::parse_interval(&to)
        .ok_or_else(|| anyhow::anyhow!("invalid target interval: {to}"))?;
    if to_ms <= from_ms { bail!("target interval must be larger than source"); }
    if to_ms % from_ms != 0 { bail!("target interval must be an even multiple of source"); }

    let start = NaiveDate::parse_from_str(&start, "%Y%m%d")?;
    let end = match &end {
        Some(e) => NaiveDate::parse_from_str(e, "%Y%m%d")?,
        None => start,
    };
    if end < start { bail!("end date must be >= start date"); }

    let coin_label = coin_arg.to_uppercase();
    let market_str = match market { Market::Perp => "perp", Market::Spot => "spot" };
    let data_dir = Path::new(DATA_DIR);

    let mut date = start;
    while date <= end {
        let date_str = date.format("%Y%m%d").to_string();
        let src_path = data_dir
            .join("candles").join(market_str).join(&coin_label)
            .join(&from).join(format!("{date_str}.csv"));

        if !src_path.exists() {
            println!("  {date_str}: no source file at {}", src_path.display());
            date += chrono::Duration::days(1);
            continue;
        }

        let source_candles = candle::read_csv(&src_path)?;
        let consolidated = candle::consolidate(&source_candles, to_ms);

        if !consolidated.is_empty() {
            let out_path = data_dir
                .join("consolidated").join(market_str).join(&coin_label)
                .join(&to).join(format!("{date_str}.csv"));
            write_candles(&out_path, &consolidated)?;
            println!("  {date_str}: {} -> {} candles ({})", source_candles.len(), consolidated.len(), out_path.display());
        }

        date += chrono::Duration::days(1);
    }
    Ok(())
}
