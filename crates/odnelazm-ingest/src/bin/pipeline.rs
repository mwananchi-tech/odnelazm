use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use futures::future;
use odnelazm::HansardScraper;
use odnelazm_ingest::{
    DataStore, IngestPipeline,
    enricher::{LmStudioSummarizer, prompts},
    postgres::PostgresStore,
    summarize::{Summarizer, SummaryContext},
};
use std::process;

#[derive(Parser)]
#[command(
    name = "odnelazm-pipeline",
    about = "odnelazm data pipeline — ingest and enrich hansard data"
)]
struct Cli {
    #[arg(
        long,
        default_value = "postgres://odnelazm:odnelazm@localhost:5432/odnelazm",
        env = "DATABASE_URL",
        help = "PostgreSQL connection string"
    )]
    database_url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scrape and store parliamentary sittings and members
    Ingest {
        /// Only ingest sittings from this date (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date)]
        start_date: Option<NaiveDate>,

        /// Only ingest sittings up to this date (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date)]
        end_date: Option<NaiveDate>,

        /// Number of concurrent scrape requests
        #[arg(long, default_value_t = 4)]
        concurrency: usize,

        /// Parliament session to import members from
        #[arg(long, default_value = "13th-parliament")]
        parliament: String,

        /// Skip scraping sittings
        #[arg(long)]
        skip_sittings: bool,

        /// Skip importing members
        #[arg(long)]
        skip_members: bool,

        /// Fetch and store individual member profile pages
        #[arg(long)]
        enrich_members: bool,

        /// Run AI speaker summaries after ingest (0 = skip)
        #[arg(long, default_value_t = 0)]
        enrich_batch: u32,
    },

    /// Generate AI summaries for bills, topics, and sittings
    Enrich {
        /// What to summarise
        target: EnrichTarget,

        /// LM Studio base URL
        #[arg(long, default_value = "http://127.0.0.1:1234")]
        llm_url: String,

        /// Model identifier as shown in LM Studio
        #[arg(long, default_value = "google/gemma-4-e4b")]
        model: String,

        /// Sampling temperature
        #[arg(long, default_value_t = 0.3)]
        temperature: f32,

        /// Number of items to fetch per DB query
        #[arg(long, default_value_t = 10)]
        batch: u32,

        /// Number of concurrent LLM requests
        #[arg(long, default_value_t = 4)]
        concurrency: usize,
    },
}

#[derive(ValueEnum, Clone)]
enum EnrichTarget {
    BillMentions,
    BillJourneys,
    BillSpeakers,
    TopicSpeakers,
    Sittings,
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("expected YYYY-MM-DD, got '{s}'"))
}

#[tokio::main]
async fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let cli = Cli::parse();

    let store = PostgresStore::connect(&cli.database_url)
        .await
        .unwrap_or_else(|e| {
            log::error!("DB connect failed: {e}");
            process::exit(1);
        });

    store.migrate().await.unwrap_or_else(|e| {
        log::error!("Migration failed: {e}");
        process::exit(1);
    });

    match cli.command {
        Command::Ingest {
            start_date,
            end_date,
            concurrency,
            parliament,
            skip_sittings,
            skip_members,
            enrich_members,
            enrich_batch,
        } => {
            run_ingest(
                store,
                start_date,
                end_date,
                concurrency,
                &parliament,
                skip_sittings,
                skip_members,
                enrich_members,
                enrich_batch,
            )
            .await
        }

        Command::Enrich {
            target,
            llm_url,
            model,
            temperature,
            batch,
            concurrency,
        } => {
            let llm = LmStudioSummarizer::new(&llm_url, &model, temperature);
            log::info!(
                "target={} batch={batch} concurrency={concurrency} model={model}",
                target.as_str()
            );
            match target {
                EnrichTarget::BillMentions => {
                    run_bill_mentions(&store, &llm, batch, concurrency, &model).await
                }
                EnrichTarget::BillJourneys => {
                    run_bill_journeys(&store, &llm, batch, concurrency, &model).await
                }
                EnrichTarget::BillSpeakers => {
                    run_bill_speakers(&store, &llm, batch, concurrency, &model).await
                }
                EnrichTarget::TopicSpeakers => {
                    run_topic_speakers(&store, &llm, batch, concurrency, &model).await
                }
                EnrichTarget::Sittings => {
                    run_sittings(&store, &llm, batch, concurrency, &model).await
                }
            }
        }
    }
}

impl EnrichTarget {
    fn as_str(&self) -> &'static str {
        match self {
            Self::BillMentions => "bill-mentions",
            Self::BillJourneys => "bill-journeys",
            Self::BillSpeakers => "bill-speakers",
            Self::TopicSpeakers => "topic-speakers",
            Self::Sittings => "sittings",
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_ingest(
    store: PostgresStore,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    concurrency: usize,
    parliament: &str,
    skip_sittings: bool,
    skip_members: bool,
    enrich_members: bool,
    enrich_batch: u32,
) {
    let scraper = HansardScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create scraper: {e}");
        process::exit(1);
    });
    let pipeline = IngestPipeline::new(scraper, store);

    if !skip_sittings {
        let stats = match (start_date, end_date) {
            (Some(start), Some(end)) => {
                log::info!("Ingesting range {start} to {end} (concurrency={concurrency})");
                pipeline.ingest_range(start, end, concurrency).await
            }
            _ => {
                log::info!("Ingesting all current sittings (concurrency={concurrency})");
                pipeline.ingest_all(concurrency).await
            }
        }
        .unwrap_or_else(|e| {
            log::error!("Pipeline error: {e}");
            process::exit(1);
        });
        log::info!("Sittings: {stats}");
    } else {
        log::info!("Sittings ingest skipped");
    }

    if !skip_members {
        let linked = pipeline
            .import_members(parliament)
            .await
            .unwrap_or_else(|e| {
                log::error!("Member import error: {e}");
                process::exit(1);
            });
        log::info!("Members: {linked} speaker-member links created");
    }

    if enrich_members {
        let enriched = pipeline
            .enrich_member_profiles(concurrency)
            .await
            .unwrap_or_else(|e| {
                log::error!("Member enrichment error: {e}");
                process::exit(1);
            });
        log::info!("Member profiles: {enriched} updated");
    }

    if enrich_batch > 0 {
        let (bills, topics) = pipeline
            .enrich_summaries(enrich_batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("Enrichment error: {e}");
                process::exit(1);
            });
        log::info!("Enrichment: {bills} bill summaries, {topics} topic summaries");
    }
}

async fn run_bill_mentions(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    model: &str,
) {
    let mut total = 0u64;
    loop {
        let pending = store
            .pending_bill_node_summaries(batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("{e}");
                vec![]
            });
        if pending.is_empty() {
            break;
        }
        for chunk in pending.chunks(concurrency) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|p| async move {
                    let prompt = prompts::bill_node_prompt(p);
                    (p.bill_mention_id, &p.bill_name, llm.complete(&prompt).await)
                })
                .collect();
            for (id, name, result) in future::join_all(tasks).await {
                match result {
                    Ok(s) => {
                        store.store_bill_node_summary(id, &s, model).await.ok();
                        total += 1;
                        log::info!("bill-mention done: {name}");
                    }
                    Err(e) => log::warn!("bill-mention failed ({name}): {e}"),
                }
            }
        }
        log::info!("bill-mentions: {total} done so far");
    }
    log::info!("bill-mentions complete — {total} summaries written");
}

async fn run_bill_journeys(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    model: &str,
) {
    let mut total = 0u64;
    loop {
        let pending = store
            .pending_bill_journey_summaries(batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("{e}");
                vec![]
            });
        if pending.is_empty() {
            break;
        }
        for chunk in pending.chunks(concurrency) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|p| async move {
                    let prompt = prompts::bill_journey_prompt(p);
                    (p.bill_id, &p.bill_name, llm.complete(&prompt).await)
                })
                .collect();
            for (id, name, result) in future::join_all(tasks).await {
                match result {
                    Ok(s) => {
                        store.store_bill_journey_summary(id, &s, model).await.ok();
                        total += 1;
                        log::info!("bill-journey done: {name}");
                    }
                    Err(e) => log::warn!("bill-journey failed ({name}): {e}"),
                }
            }
        }
        log::info!("bill-journeys: {total} done so far");
    }
    log::info!("bill-journeys complete — {total} summaries written");
}

async fn run_bill_speakers(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    model: &str,
) {
    let mut total = 0u64;
    loop {
        let pending = store
            .pending_bill_summaries(batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("{e}");
                vec![]
            });
        if pending.is_empty() {
            break;
        }
        for chunk in pending.chunks(concurrency) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|p| {
                    let ctx = SummaryContext {
                        member_name: p.member_name.clone().unwrap_or_else(|| "Unknown".into()),
                        title: p.bill_name.clone(),
                        item_type: "bill".into(),
                        stage: p.stage.clone(),
                        date: p.date,
                        house: p.house.clone(),
                    };
                    async move {
                        (
                            p.bill_mention_id,
                            p.speaker_id,
                            llm.summarize(&ctx, &p.contributions_text).await,
                        )
                    }
                })
                .collect();
            for (mention_id, speaker_id, result) in future::join_all(tasks).await {
                match result {
                    Ok(s) => {
                        store
                            .store_bill_mention_summary(mention_id, speaker_id, &s, model)
                            .await
                            .ok();
                        total += 1;
                    }
                    Err(e) => log::warn!("bill-speaker summary failed: {e}"),
                }
            }
        }
        log::info!("bill-speakers: {total} done so far");
    }
    log::info!("bill-speakers complete — {total} summaries written");
}

async fn run_topic_speakers(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    model: &str,
) {
    let mut total = 0u64;
    loop {
        let pending = store
            .pending_topic_summaries(batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("{e}");
                vec![]
            });
        if pending.is_empty() {
            break;
        }
        for chunk in pending.chunks(concurrency) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|p| {
                    let ctx = SummaryContext {
                        member_name: p.member_name.clone().unwrap_or_else(|| "Unknown".into()),
                        title: p.topic_title.clone(),
                        item_type: "topic".into(),
                        stage: None,
                        date: p.date,
                        house: p.house.clone(),
                    };
                    async move {
                        (
                            p.topic_id,
                            p.speaker_id,
                            llm.summarize(&ctx, &p.contributions_text).await,
                        )
                    }
                })
                .collect();
            for (topic_id, speaker_id, result) in future::join_all(tasks).await {
                match result {
                    Ok(s) => {
                        store
                            .store_topic_summary(topic_id, speaker_id, &s, model)
                            .await
                            .ok();
                        total += 1;
                    }
                    Err(e) => log::warn!("topic-speaker summary failed: {e}"),
                }
            }
        }
        log::info!("topic-speakers: {total} done so far");
    }
    log::info!("topic-speakers complete — {total} summaries written");
}

async fn run_sittings(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    model: &str,
) {
    let mut total = 0u64;
    loop {
        let pending = store
            .pending_sitting_summaries(batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("{e}");
                vec![]
            });
        if pending.is_empty() {
            break;
        }
        let eff = concurrency.min(2);
        for chunk in pending.chunks(eff) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|p| async move {
                    let prompt = prompts::sitting_prompt(p);
                    (p.sitting_id, p.date, &p.house, llm.complete(&prompt).await)
                })
                .collect();
            for (id, date, house, result) in future::join_all(tasks).await {
                match result {
                    Ok(s) => {
                        store
                            .store_sitting_generated_summary(id, &s, model)
                            .await
                            .ok();
                        total += 1;
                        log::info!("sitting done: {date} {house}");
                    }
                    Err(e) => log::warn!("sitting failed ({date} {house}): {e}"),
                }
            }
        }
        log::info!("sittings: {total} done so far");
    }
    log::info!("sittings complete — {total} summaries written");
}
