use std::collections::{HashMap, HashSet};
use std::process;

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use futures::stream::{FuturesUnordered, StreamExt};
use log::LevelFilter;
use odnelazm::scraper::WebScraper;
use odnelazm::types::House;

#[derive(Parser)]
#[command(name = "odnelazm")]
#[command(about = "A mzalendo.com hansard scraper", long_about = None)]
struct Cli {
    #[arg(
        short = 'l',
        long = "log-level",
        value_enum,
        default_value = "info",
        global = true,
        help = "Set the logging level"
    )]
    log_level: LogLevel,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, ValueEnum)]
enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// List available parliamentary sittings with optional filtering and pagination
    List {
        #[arg(long, help = "Maximum number of results to return")]
        limit: Option<usize>,

        #[arg(long, help = "Number of results to skip from the beginning")]
        offset: Option<usize>,

        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Filter sessions from this date onwards"
        )]
        start_date: Option<String>,

        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Filter sessions up to this date"
        )]
        end_date: Option<String>,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    /// Fetch the full transcript of a sitting including sections, contributions and procedural notes
    Detail {
        #[arg(help = "URL of the hansard detail page to fetch")]
        url: String,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,

        #[arg(long, help = "Fetch speaker details from person profile pages")]
        fetch_speakers: bool,
    },
}

type FilterResult = Result<
    (
        Option<usize>,
        Option<usize>,
        Option<NaiveDate>,
        Option<NaiveDate>,
    ),
    String,
>;

fn parse_date(date_str: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date format '{}'. Expected YYYY-MM-DD", date_str))
}

fn validate_and_parse_filters(
    limit: Option<usize>,
    offset: Option<usize>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> FilterResult {
    let start = start_date.as_deref().map(parse_date).transpose()?;
    let end = end_date.as_deref().map(parse_date).transpose()?;

    let Some(s) = start else {
        return Ok((limit, offset, None, None));
    };
    let Some(e) = end else {
        return Ok((limit, offset, start, None));
    };
    if s > e {
        return Err(format!("Start date ({s}) cannot be after end date ({e})"));
    }
    if offset.is_some_and(|o| o == 0) {
        return Err("Offset must be greater than 0".to_string());
    }
    if limit.is_some_and(|l| l == 0) {
        return Err("Limit must be greater than 0".to_string());
    }

    Ok((limit, offset, start, end))
}

fn serialize_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{}", json),
        Err(e) => {
            log::error!("Error serializing to JSON: {}", e);
            process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.log_level.clone().into())
        .init();

    let scraper = WebScraper::new().unwrap_or_else(|e| {
        log::error!("Error creating scraper: {}", e);
        process::exit(1);
    });

    match cli.command {
        Commands::List {
            limit,
            offset,
            start_date,
            end_date,
            format,
        } => {
            let (limit, offset, start_date, end_date) = validate_and_parse_filters(
                limit, offset, start_date, end_date,
            )
            .unwrap_or_else(|e| {
                log::error!("Invalid args: {}", e);
                process::exit(1);
            });

            log::info!("Fetching hansard list from https://info.mzalendo.com/hansard/...");

            let mut listings = scraper.fetch_hansard_list().await.unwrap_or_else(|e| {
                log::error!("Error fetching hansard list: {}", e);
                process::exit(1);
            });

            if let Some(start) = start_date {
                listings.retain(|l| l.date >= start);
            }
            if let Some(end) = end_date {
                listings.retain(|l| l.date <= end);
            }

            if let Some(off) = offset {
                if off >= listings.len() {
                    log::error!(
                        "Offset ({}) is greater than or equal to available results ({})",
                        off,
                        listings.len()
                    );
                    process::exit(1);
                }
                listings = listings.into_iter().skip(off).collect();
            }

            if let Some(lim) = limit {
                listings.truncate(lim);
            }

            match format {
                OutputFormat::Json => serialize_json(&listings),
                OutputFormat::Text => {
                    if listings.is_empty() {
                        println!("No entries to display.");
                    } else {
                        for (i, listing) in listings.iter().enumerate() {
                            println!("{:>3}. {}", i + 1, listing);
                        }

                        let senate_count =
                            listings.iter().filter(|l| l.house == House::Senate).count();
                        let assembly_count = listings
                            .iter()
                            .filter(|l| l.house == House::NationalAssembly)
                            .count();

                        println!("\nStatistics:");
                        println!("  Senate sittings:            {}", senate_count);
                        println!("  National Assembly sittings: {}", assembly_count);
                        println!("  Total:                      {}", listings.len());
                    }
                }
            }
        }

        Commands::Detail {
            url,
            format,
            fetch_speakers,
        } => {
            log::info!("Fetching hansard detail from {}...", url);

            let mut detail = scraper
                .fetch_hansard_detail(&url)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching hansard detail: {}", e);
                    process::exit(1);
                });

            if fetch_speakers {
                let speaker_urls: HashSet<String> = detail
                    .sections
                    .iter()
                    .flat_map(|s| &s.contributions)
                    .filter_map(|c| c.speaker_url.as_ref())
                    .cloned()
                    .collect();

                if !speaker_urls.is_empty() {
                    log::info!("Fetching {} speaker profiles...", speaker_urls.len());

                    let mut futures: FuturesUnordered<_> = speaker_urls
                        .iter()
                        .map(|url| {
                            let scraper = &scraper;
                            async move { (url, scraper.fetch_person_details(url).await) }
                        })
                        .collect();

                    let mut speaker_map = HashMap::new();
                    while let Some((url, result)) = futures.next().await {
                        match result {
                            Ok(details) => {
                                speaker_map.insert(url.clone(), details);
                            }
                            Err(e) => log::warn!("Failed to fetch speaker {}: {}", url, e),
                        }
                    }

                    for contrib in detail
                        .sections
                        .iter_mut()
                        .flat_map(|s| &mut s.contributions)
                    {
                        if let Some(url) = &contrib.speaker_url {
                            contrib.speaker_details = speaker_map.get(url).cloned();
                        }
                    }

                    log::info!(
                        "Successfully fetched {} speaker profiles",
                        speaker_map.len()
                    );
                }
            } else {
                log::warn!("Fetching speakers skipped for {:?} format", format);
            }

            match format {
                OutputFormat::Json => serialize_json(&detail),
                OutputFormat::Text => println!("{}", detail),
            }
        }
    }
}
