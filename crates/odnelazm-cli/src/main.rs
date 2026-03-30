use std::process;
use std::str::FromStr;

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use log::LevelFilter;
use odnelazm::{HansardScraper, House, SittingListOptions};
use polars::prelude::*;

#[derive(Parser)]
#[command(name = "odnelazm")]
#[command(about = "Kenya Hansard scraper — automatically routes to archive or current source based on date", long_about = None)]
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
    Json,
    Csv,
    Parquet,
}

#[derive(Subcommand)]
enum Commands {
    /// List parliamentary sittings with automatic source routing.
    ///
    /// Routing rules (cutoff = 2013-03-28):
    ///   No dates              → current source, paged via --page / --all
    ///   --end-date < cutoff   → archive only
    ///   --start-date ≥ cutoff → current only, paged via --page / --all
    ///   Range spans cutoff    → BOTH sources fetched in parallel and merged by date;
    ///                           --page and --all are ignored, use --limit / --offset instead
    ///
    /// Examples:
    ///   # Recent sittings (current, page 1)
    ///   odnelazm sittings
    ///
    ///   # All archive sittings in 2010
    ///   odnelazm sittings --start-date 2010-01-01 --end-date 2010-12-31
    ///
    ///   # Cross-era range: archive + current merged
    ///   odnelazm sittings --start-date 2012-01-01 --end-date 2014-12-31
    Sittings {
        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Start of date range. If before 2013-03-28 with no --end-date, both archive and current are queried and merged.",
            value_parser = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string()),
        )]
        start_date: Option<NaiveDate>,

        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "End of date range. Before 2013-03-28 → archive only; on or after → current (or both if --start-date is also before the cutoff).",
            value_parser = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| e.to_string()),
        )]
        end_date: Option<NaiveDate>,

        #[arg(
            long,
            value_parser = |s: &str| House::from_str(s).map_err(|e| e.to_string()),
            help = "Filter by house (senate, national_assembly, na)"
        )]
        house: Option<House>,

        #[arg(
            long,
            help = "Page to fetch from the current source (ignored when --all is set or when a cross-era range triggers a merged query)",
            default_value = "1",
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        page: u32,

        #[arg(
            long,
            help = "Fetch all pages from the current source at once (ignored for cross-era merged queries; conflicts with --page)",
            conflicts_with = "page"
        )]
        all: bool,

        #[arg(
            long,
            help = "Maximum results to return, applied after merging and sorting",
            value_parser = |s: &str| s.parse::<usize>().map_err(|e| e.to_string()).and_then(|v| if v > 0 { Ok(v) } else { Err("must be greater than 0".into()) }),
        )]
        limit: Option<usize>,

        #[arg(
            long,
            help = "Results to skip, applied after merging and sorting",
            value_parser = |s: &str| s.parse::<usize>().map_err(|e| e.to_string()).and_then(|v| if v > 0 { Ok(v) } else { Err("must be greater than 0".into()) }),
        )]
        offset: Option<usize>,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "json",
            help = "Output format"
        )]
        format: OutputFormat,
    },

    /// Fetch the full transcript of a sitting. Source is detected automatically from the URL.
    ///
    /// Archive URLs: https://info.mzalendo.com/hansard/sitting/...
    /// Current URLs: https://mzalendo.com/democracy-tools/hansard/...
    Sitting {
        #[arg(help = "URL or slug of the sitting to fetch")]
        url_or_slug: String,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "json",
            help = "Output format"
        )]
        format: OutputFormat,
    },

    /// List members of parliament (current source only)
    Members {
        #[arg(
            help = "House to list members for (senate, national_assembly, na)",
            value_parser = |s: &str| House::from_str(s).map_err(|e| e.to_string()),
        )]
        house: House,

        #[arg(help = "Parliament session (e.g. 13th-parliament, 12th-parliament)")]
        parliament: String,

        #[arg(
            long,
            help = "Page number to fetch (ignored when --all is set)",
            default_value = "1",
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        page: u32,

        #[arg(long, help = "Fetch all pages at once", conflicts_with = "page")]
        all: bool,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "json",
            help = "Output format"
        )]
        format: OutputFormat,
    },

    /// List all members from both houses in parallel (current source only)
    AllMembers {
        #[arg(
            help = "Parliament session (e.g. 13th-parliament, 12th-parliament)",
            default_value = "13th-parliament"
        )]
        parliament: String,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "json",
            help = "Output format"
        )]
        format: OutputFormat,
    },

    /// Fetch a member's full profile including speeches, bills, and voting record (current source only)
    Profile {
        #[arg(help = "URL or slug of the member profile to fetch")]
        url_or_slug: String,

        #[arg(long, help = "Fetch all pages of parliamentary activity")]
        all_activity: bool,

        #[arg(long, help = "Fetch all pages of sponsored bills")]
        all_bills: bool,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "json",
            help = "Output format"
        )]
        format: OutputFormat,
    },
}

fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{}", json),
        Err(e) => {
            log::error!("Serialization error: {}", e);
            process::exit(1);
        }
    }
}

/// Convert any serializable value to a polars DataFrame via JSON.
/// Single objects are wrapped in an array so polars always sees a record list.
fn to_dataframe<T: serde::Serialize>(data: &T) -> PolarsResult<DataFrame> {
    let mut value =
        serde_json::to_value(data).map_err(|e| PolarsError::ComputeError(e.to_string().into()))?;
    if !value.is_array() {
        value = serde_json::Value::Array(vec![value]);
    }
    let json = serde_json::to_string(&value)
        .map_err(|e| PolarsError::ComputeError(e.to_string().into()))?;
    JsonReader::new(std::io::Cursor::new(json.into_bytes())).finish()
}

fn print_csv<T: serde::Serialize>(data: &T) {
    let mut df = to_dataframe(data).unwrap_or_else(|e| {
        log::error!("Failed to build dataframe: {}", e);
        process::exit(1);
    });
    CsvWriter::new(std::io::stdout())
        .finish(&mut df)
        .unwrap_or_else(|e| {
            log::error!("CSV write error: {}", e);
            process::exit(1);
        });
}

fn print_parquet<T: serde::Serialize>(data: &T) {
    let mut df = to_dataframe(data).unwrap_or_else(|e| {
        log::error!("Failed to build dataframe: {}", e);
        process::exit(1);
    });
    ParquetWriter::new(std::io::stdout())
        .finish(&mut df)
        .unwrap_or_else(|e| {
            log::error!("Parquet write error: {}", e);
            process::exit(1);
        });
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.log_level.into())
        .init();

    let scraper = HansardScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create scraper: {}", e);
        process::exit(1);
    });

    match cli.command {
        Commands::Sittings {
            start_date,
            end_date,
            house,
            page,
            all,
            limit,
            offset,
            format,
        } => {
            if let Some(start) = start_date
                && let Some(end) = end_date
                && start > end
            {
                log::error!("--start-date cannot be after --end-date");
                process::exit(1);
            }

            let listings = scraper
                .list_sittings(SittingListOptions {
                    start_date,
                    end_date,
                    house,
                    page,
                    all,
                    limit,
                    offset,
                })
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching sittings: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&listings),
                OutputFormat::Csv => print_csv(&listings),
                OutputFormat::Parquet => print_parquet(&listings),
            }
        }

        Commands::Sitting {
            url_or_slug,
            format,
        } => {
            let sitting = scraper.get_sitting(&url_or_slug).await.unwrap_or_else(|e| {
                log::error!("Error fetching sitting: {}", e);
                process::exit(1);
            });

            match format {
                OutputFormat::Json => print_json(&sitting),
                OutputFormat::Csv => print_csv(&sitting),
                OutputFormat::Parquet => print_parquet(&sitting),
            }
        }

        Commands::Members {
            house,
            parliament,
            page,
            all,
            format,
        } => {
            let members = if all {
                scraper.list_all_members(house, &parliament).await
            } else {
                scraper.list_members(house, &parliament, page).await
            }
            .unwrap_or_else(|e| {
                log::error!("Error fetching members: {}", e);
                process::exit(1);
            });

            match format {
                OutputFormat::Json => print_json(&members),
                OutputFormat::Csv => print_csv(&members),
                OutputFormat::Parquet => print_parquet(&members),
            }
        }

        Commands::AllMembers { parliament, format } => {
            let members = scraper
                .list_all_members_all_houses(&parliament)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching all members: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&members),
                OutputFormat::Csv => print_csv(&members),
                OutputFormat::Parquet => print_parquet(&members),
            }
        }

        Commands::Profile {
            url_or_slug,
            all_activity,
            all_bills,
            format,
        } => {
            let profile = scraper
                .get_member_profile(&url_or_slug, all_activity, all_bills)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching member profile: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&profile),
                OutputFormat::Csv => print_csv(&profile),
                OutputFormat::Parquet => print_parquet(&profile),
            }
        }
    }
}
