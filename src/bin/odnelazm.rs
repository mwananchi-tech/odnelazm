use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use odnelazm::scraper::WebScraper;
use std::process;

#[derive(Parser)]
#[command(name = "odnelazm")]
#[command(about = "A mzalendo.com hansard scraper", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Commands {
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
            help = "Output format (text or json)"
        )]
        format: OutputFormat,
    },
    Detail {
        #[arg(help = "URL of the hansard detail page to fetch")]
        url: String,

        #[arg(
            short = 'o',
            long = "output",
            value_enum,
            default_value = "text",
            help = "Output format (text or json)"
        )]
        format: OutputFormat,

        #[arg(
            long,
            help = "Fetch speaker details from person profile pages (recursive)"
        )]
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
    let start = if let Some(ref s) = start_date {
        Some(parse_date(s)?)
    } else {
        None
    };

    let end = if let Some(ref e) = end_date {
        Some(parse_date(e)?)
    } else {
        None
    };

    if let (Some(start), Some(end)) = (start, end)
        && start > end
    {
        return Err(format!(
            "Start date ({}) cannot be after end date ({})",
            start, end
        ));
    }

    if let Some(off) = offset
        && off == 0
    {
        return Err("Offset must be greater than 0".to_string());
    }

    if let Some(lim) = limit
        && lim == 0
    {
        return Err("Limit must be greater than 0".to_string());
    }

    Ok((limit, offset, start, end))
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List {
            limit,
            offset,
            start_date,
            end_date,
            format,
        } => {
            let (limit, offset, start_date, end_date) =
                match validate_and_parse_filters(limit, offset, start_date, end_date) {
                    Ok(filters) => filters,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(1);
                    }
                };

            let scraper = match WebScraper::new() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error creating scraper: {}", e);
                    process::exit(1);
                }
            };

            println!("Fetching hansard list from https://info.mzalendo.com/hansard/...");

            let mut listings = match scraper.fetch_hansard_list().await {
                Ok(listings) => listings,
                Err(e) => {
                    eprintln!("Error fetching hansard list: {}", e);
                    process::exit(1);
                }
            };

            let total_fetched = listings.len();

            if let Some(start) = start_date {
                listings.retain(|l| l.date >= start);
            }

            if let Some(end) = end_date {
                listings.retain(|l| l.date <= end);
            }

            let after_date_filter = listings.len();

            if let Some(off) = offset {
                if off >= listings.len() {
                    eprintln!(
                        "Error: Offset ({}) is greater than or equal to available results ({})",
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
                OutputFormat::Json => match serde_json::to_string_pretty(&listings) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("Error serializing to JSON: {}", e);
                        process::exit(1);
                    }
                },
                OutputFormat::Text => {
                    println!("Successfully fetched {} hansard listings", total_fetched);
                    if start_date.is_some() || end_date.is_some() {
                        println!("After date filtering: {} listings", after_date_filter);
                    }
                    if offset.is_some() || limit.is_some() {
                        println!("After pagination: {} listings", listings.len());
                    }
                    println!();

                    if !listings.is_empty() {
                        println!("Entries:");
                        for (i, listing) in listings.iter().enumerate() {
                            println!(
                                "{}. {} - {} ({})",
                                i + 1,
                                listing.house_name(),
                                listing.date,
                                listing.display_text
                            );

                            if let Some(start) = listing.start_time {
                                print!("   Start: {}", start);
                                if let Some(end) = listing.end_time {
                                    println!(" | End: {}", end);
                                } else {
                                    println!();
                                }
                            }
                        }
                    } else {
                        println!("No entries to display.");
                    }

                    let senate_count = listings
                        .iter()
                        .filter(|l| matches!(l.house, odnelazm::types::House::Senate))
                        .count();
                    let assembly_count = listings
                        .iter()
                        .filter(|l| matches!(l.house, odnelazm::types::House::NationalAssembly))
                        .count();

                    println!("\nStatistics:");
                    println!("  Senate sittings: {}", senate_count);
                    println!("  National Assembly sittings: {}", assembly_count);
                    println!("  Total: {}", listings.len());
                }
            }
        }
        Commands::Detail {
            url,
            format,
            fetch_speakers,
        } => {
            let scraper = match WebScraper::new() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error creating scraper: {}", e);
                    process::exit(1);
                }
            };

            println!("Fetching hansard detail from {}...", url);

            let detail = match scraper.fetch_hansard_detail_parsed(&url).await {
                Ok(detail) => detail,
                Err(e) => {
                    eprintln!("Error fetching hansard detail: {}", e);
                    process::exit(1);
                }
            };

            if fetch_speakers {
                println!("Note: Speaker detail fetching not yet implemented");
            }

            match format {
                OutputFormat::Json => match serde_json::to_string_pretty(&detail) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("Error serializing to JSON: {}", e);
                        process::exit(1);
                    }
                },
                OutputFormat::Text => {
                    println!("\n=== HANSARD DETAIL ===");
                    println!("House: {}", detail.house.to_string());
                    println!("Date: {}", detail.date);
                    if let Some(start) = detail.start_time {
                        println!("Start Time: {}", start);
                    }
                    println!("Parliament: {}", detail.parliament_number);
                    println!("Session: {}", detail.session_number);
                    println!("Session Type: {}", detail.session_type);
                    println!("Speaker in Chair: {}", detail.speaker_in_chair);
                    println!("\n=== SECTIONS ({}) ===", detail.sections.len());

                    for (i, section) in detail.sections.iter().enumerate() {
                        println!("\n[{}] {}", i + 1, section.section_type);
                        if !section.contributions.is_empty() {
                            println!("  Contributions: {}", section.contributions.len());

                            for (j, contrib) in section.contributions.iter().take(3).enumerate() {
                                println!("\n  [Contribution {}]", j + 1);
                                println!("    Speaker: {}", contrib.speaker_name);
                                if let Some(role) = &contrib.speaker_role {
                                    println!("    Role: {}", role);
                                }
                                if let Some(url) = &contrib.speaker_url {
                                    println!("    Profile URL: {}", url);
                                }
                                let preview = if contrib.content.len() > 150 {
                                    format!("{}...", &contrib.content[..150])
                                } else {
                                    contrib.content.clone()
                                };
                                println!("    Content: {}", preview);
                                if !contrib.procedural_notes.is_empty() {
                                    println!(
                                        "    Procedural notes: {}",
                                        contrib.procedural_notes.len()
                                    );
                                }
                            }

                            if section.contributions.len() > 3 {
                                println!(
                                    "\n  ... and {} more contributions",
                                    section.contributions.len() - 3
                                );
                            }
                        }
                    }

                    let total_contributions: usize =
                        detail.sections.iter().map(|s| s.contributions.len()).sum();
                    let speakers_with_urls = detail
                        .sections
                        .iter()
                        .flat_map(|s| &s.contributions)
                        .filter(|c| c.speaker_url.is_some())
                        .count();

                    println!("\n=== STATISTICS ===");
                    println!("Total sections: {}", detail.sections.len());
                    println!("Total contributions: {}", total_contributions);
                    println!("Speakers with profile URLs: {}", speakers_with_urls);
                }
            }
        }
    }
}
