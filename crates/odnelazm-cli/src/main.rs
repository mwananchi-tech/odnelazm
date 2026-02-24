use std::process;
use std::str::FromStr;

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use log::LevelFilter;
use odnelazm::{
    House,
    archive::{
        WebScraper as ArchiveScraper,
        utils::{ListingFilter, ListingStats},
    },
    current::WebScraper as CurrentScraper,
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
    /// Archive hansard data from info.mzalendo.com
    Archive {
        #[command(subcommand)]
        command: ArchiveCommands,
    },
    /// Current hansard data from mzalendo.com/democracy-tools
    Current {
        #[command(subcommand)]
        command: CurrentCommands,
    },
}

#[derive(Subcommand)]
enum ArchiveCommands {
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
            long,
            value_parser = |s: &str| House::from_str(s).map_err(|e| e.to_string()),
            help = "Filter by house (senate, national_assembly, na)"
        )]
        house: Option<House>,

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
    Sitting {
        #[arg(help = "URL of the hansard detail page to fetch")]
        url: String,

        #[arg(long, help = "Fetch speaker details from person profile pages")]
        fetch_speakers: bool,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum CurrentCommands {
    /// List available sittings (paged or all at once)
    Sittings {
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
            long,
            value_parser = |s: &str| House::from_str(s).map_err(|e| e.to_string()),
            help = "Filter by house (senate, national_assembly, na)"
        )]
        house: Option<House>,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    /// Fetch the full transcript of a sitting
    Sitting {
        #[arg(help = "URL or slug of the sitting to fetch")]
        url_or_slug: String,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    /// List members of parliament
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
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    /// List all members from both houses in parallel
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
            default_value = "text",
            help = "Output format"
        )]
        format: OutputFormat,
    },
    /// Fetch a member's full profile including speeches, bills, and voting record
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
            default_value = "text",
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.log_level.clone().into())
        .init();

    match cli.command {
        Commands::Archive { command } => run_archive(command).await,
        Commands::Current { command } => run_current(command).await,
    }
}

async fn run_archive(command: ArchiveCommands) {
    let scraper = ArchiveScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create archive scraper: {}", e);
        process::exit(1);
    });

    match command {
        ArchiveCommands::List {
            limit,
            offset,
            start_date,
            end_date,
            house,
            format,
        } => {
            let filters = ListingFilter {
                limit,
                offset,
                start_date,
                end_date,
                house,
            }
            .validate()
            .unwrap_or_else(|e| {
                log::error!("Invalid args: {}", e);
                process::exit(1);
            });

            let mut listings = scraper.fetch_hansard_list().await.unwrap_or_else(|e| {
                log::error!("Error fetching hansard list: {}", e);
                process::exit(1);
            });

            listings = filters.apply(listings);

            match format {
                OutputFormat::Json => print_json(&listings),
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

        ArchiveCommands::Sitting {
            url,
            fetch_speakers,
            format,
        } => {
            let detail = scraper
                .fetch_hansard_sitting(&url, fetch_speakers)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching hansard detail: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&detail),
                OutputFormat::Text => println!("{}", detail),
            }
        }
    }
}

async fn run_current(command: CurrentCommands) {
    let scraper = CurrentScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create current scraper: {}", e);
        process::exit(1);
    });

    match command {
        CurrentCommands::Sittings {
            page,
            all,
            house,
            format,
        } => {
            let listings = if all {
                scraper.fetch_all_sittings(house).await
            } else {
                scraper.fetch_hansard_list(page, house).await
            }
            .unwrap_or_else(|e| {
                log::error!("Error fetching sittings: {}", e);
                process::exit(1);
            });

            match format {
                OutputFormat::Json => print_json(&listings),
                OutputFormat::Text => {
                    if listings.is_empty() {
                        println!("No entries to display.");
                    } else {
                        for (i, listing) in listings.iter().enumerate() {
                            println!("{:>3}. {}", i + 1, listing);
                        }
                    }
                }
            }
        }

        CurrentCommands::Sitting {
            url_or_slug,
            format,
        } => {
            let sitting = scraper
                .fetch_hansard_sitting(&url_or_slug)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching sitting: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&sitting),
                OutputFormat::Text => println!("{}", sitting),
            }
        }

        CurrentCommands::Members {
            house,
            parliament,
            page,
            all,
            format,
        } => {
            let members = if all {
                scraper.fetch_all_members(house, &parliament).await
            } else {
                scraper.fetch_members(house, &parliament, page).await
            }
            .unwrap_or_else(|e| {
                log::error!("Error fetching members: {}", e);
                process::exit(1);
            });

            match format {
                OutputFormat::Json => print_json(&members),
                OutputFormat::Text => {
                    if members.is_empty() {
                        println!("No members to display.");
                    } else {
                        for (i, member) in members.iter().enumerate() {
                            println!("{:>3}. {}", i + 1, member);
                        }
                    }
                }
            }
        }

        CurrentCommands::AllMembers { parliament, format } => {
            let members = scraper
                .fetch_all_members_all_houses(&parliament)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching all members: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&members),
                OutputFormat::Text => {
                    if members.is_empty() {
                        println!("No members to display.");
                    } else {
                        for (i, member) in members.iter().enumerate() {
                            println!("{:>3}. {}", i + 1, member);
                        }
                    }
                }
            }
        }

        CurrentCommands::Profile {
            url_or_slug,
            all_activity,
            all_bills,
            format,
        } => {
            let profile = scraper
                .fetch_member_profile(&url_or_slug, all_activity, all_bills)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Error fetching member profile: {}", e);
                    process::exit(1);
                });

            match format {
                OutputFormat::Json => print_json(&profile),
                OutputFormat::Text => println!("{}", profile),
            }
        }
    }
}
