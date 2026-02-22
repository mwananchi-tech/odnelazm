use std::process;
use std::str::FromStr;

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use log::LevelFilter;
use odnelazm::archive::{
    WebScraper,
    types::House,
    utils::{ListingFilter, ListingStats},
};

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
        #[arg(
            long,
            help = "Maximum number of results to return",
            value_parser = clap::value_parser!(u16).range(1..)
        )]
        limit: Option<usize>,

        #[arg(
            long,
            help = "Number of results to skip from the beginning",
            value_parser = clap::value_parser!(u16).range(1..)
        )]
        offset: Option<usize>,

        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Filter sessions from this date onwards",
            value_parser = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string()),
        )]
        start_date: Option<NaiveDate>,

        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Filter sessions up to this date",
            value_parser = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string()),
        )]
        end_date: Option<NaiveDate>,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,

        #[arg(
            long,
            value_parser = |s: &str| House::from_str(s).map_err(|e| e.to_string()),
            help = "Filter by house"
        )]
        house: Option<House>,
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
            house,
        } => {
            let listing_filters = ListingFilter {
                limit,
                offset,
                start_date,
                end_date,
                house,
            };

            let listing_filters = listing_filters.validate().unwrap_or_else(|e| {
                log::error!("Invalid args: {e}");
                process::exit(1);
            });

            let mut listings = scraper.fetch_hansard_list().await.unwrap_or_else(|e| {
                log::error!("Error fetching hansard list: {}", e);
                process::exit(1);
            });

            listings = listing_filters.apply(listings);

            match format {
                OutputFormat::Json => serialize_json(&listings),
                OutputFormat::Text => {
                    if listings.is_empty() {
                        println!("No entries to display.");
                    } else {
                        for (i, listing) in listings.iter().enumerate() {
                            println!("{:>3}. {}", i + 1, listing);
                        }
                        print!("{}", ListingStats::from_hansard_listings(&listings));
                    }
                }
            }
        }

        Commands::Detail {
            url,
            format,
            fetch_speakers,
        } => {
            let detail = scraper
                .fetch_hansard_detail(&url, fetch_speakers)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching hansard detail: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => serialize_json(&detail),
                OutputFormat::Text => println!("{}", detail),
            }
        }
    }
}
