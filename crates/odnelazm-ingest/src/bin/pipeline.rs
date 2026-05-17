use chrono::NaiveDate;
use clap::{Args, Parser, Subcommand, ValueEnum};
use futures::future;
use odnelazm::HansardScraper;
use odnelazm_ingest::{
    DataStore, IngestPipeline,
    enricher::{LmStudioSummarizer, prompts},
    metrics::{MetricsSink, NoopSink, prometheus::PrometheusPushSink},
    postgres::PostgresStore,
    summarize::{Summarizer, SummaryContext},
};
use std::{fmt::Display, process};

#[derive(Parser)]
#[command(
    name = "odnelazm-pipeline",
    about = "odnelazm data pipeline for ingesting and enriching hansard data"
)]
struct Cli {
    #[arg(
        long,
        default_value = "postgres://odnelazm:odnelazm@localhost:5432/odnelazm",
        env = "DATABASE_URL",
        help = "PostgreSQL connection string"
    )]
    database_url: String,

    /// Prometheus pushgateway URL. When set, pipeline metrics are pushed after
    /// each batch. Omitting this flag disables metrics — ingestion is unaffected.
    #[arg(long, env = "METRICS_URL")]
    metrics_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scrape and store parliamentary sittings and members
    Ingest(IngestCmd),
    /// Generate AI summaries for bills, topics, and sittings
    Enrich(EnrichCmd),
}

#[derive(Args)]
struct IngestCmd {
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
}

impl IngestCmd {
    async fn run(&self, pipeline: &IngestPipeline<PostgresStore>) {
        if !self.skip_sittings {
            let stats = match (self.start_date, self.end_date) {
                (Some(start), Some(end)) => {
                    log::info!(
                        "Ingesting range {start} to {end} (concurrency={})",
                        self.concurrency
                    );
                    pipeline.ingest_range(start, end, self.concurrency).await
                }
                _ => {
                    log::info!(
                        "Ingesting all current sittings (concurrency={})",
                        self.concurrency
                    );
                    pipeline.ingest_all(self.concurrency).await
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

        if !self.skip_members {
            let linked = pipeline
                .import_members(&self.parliament)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Member import error: {e}");
                    process::exit(1);
                });
            log::info!("Members: {linked} speaker-member links created");
        }

        if self.enrich_members {
            let enriched = pipeline
                .enrich_member_profiles(self.concurrency)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Member enrichment error: {e}");
                    process::exit(1);
                });
            log::info!("Member profiles: {enriched} updated");
        }

        if self.enrich_batch > 0 {
            let (bills, topics) = pipeline
                .enrich_summaries(self.enrich_batch)
                .await
                .unwrap_or_else(|e| {
                    log::error!("Enrichment error: {e}");
                    process::exit(1);
                });
            log::info!("Enrichment: {bills} bill summaries, {topics} topic summaries");
        }
    }
}

#[derive(Args)]
struct EnrichCmd {
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
}

impl EnrichCmd {
    async fn run(&self, pipeline: &IngestPipeline<PostgresStore>) {
        let store = pipeline.store();
        let summarizer = pipeline.summarizer.as_deref().unwrap_or_else(|| {
            log::error!("No summarizer configured");
            process::exit(1);
        });
        let noop = NoopSink;
        let metrics: &dyn MetricsSink = pipeline.metrics.as_deref().unwrap_or(&noop);

        log::info!(
            "target={} batch={} concurrency={} model={}",
            self.target,
            self.batch,
            self.concurrency,
            self.model
        );

        match self.target {
            EnrichTarget::BillMentions => self.run_bill_mentions(store, summarizer, metrics).await,
            EnrichTarget::BillJourneys => self.run_bill_journeys(store, summarizer, metrics).await,
            EnrichTarget::BillSpeakers => self.run_bill_speakers(store, summarizer, metrics).await,
            EnrichTarget::Topics => self.run_topics(store, summarizer, metrics).await,
            EnrichTarget::TopicSpeakers => {
                self.run_topic_speakers(store, summarizer, metrics).await
            }
            EnrichTarget::Sittings => self.run_sittings(store, summarizer, metrics).await,
        }
    }

    async fn run_bill_mentions(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_bill_appearance_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            for chunk in pending.chunks(self.concurrency) {
                let tasks: Vec<_> = chunk
                    .iter()
                    .map(|p| async move {
                        let prompt = prompts::bill_appearance_prompt(p);
                        (
                            p.bill_mention_id,
                            &p.bill_name,
                            summarizer.summarize(&prompt).await,
                        )
                    })
                    .collect();
                for (id, name, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_bill_appearance_summary(id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "bill-mentions"), ("model", &self.model)],
                            );
                            log::info!("bill-mention done: {name}");
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "bill-mentions")]);
                            log::warn!("bill-mention failed ({name}): {e}");
                        }
                    }
                }
            }
            metrics.gauge("summaries_pending", 0.0, &[("target", "bill-mentions")]);
            metrics.flush().await;
            log::info!("bill-mentions: {total} done so far");
        }
        log::info!("bill-mentions complete: {total} summaries written");
    }

    async fn run_bill_journeys(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_bill_journey_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            for chunk in pending.chunks(self.concurrency) {
                let tasks: Vec<_> = chunk
                    .iter()
                    .map(|p| async move {
                        let prompt = prompts::bill_journey_prompt(p);
                        (p.bill_id, &p.bill_name, summarizer.summarize(&prompt).await)
                    })
                    .collect();
                for (id, name, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_bill_journey_summary(id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "bill-journeys"), ("model", &self.model)],
                            );
                            log::info!("bill-journey done: {name}");
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "bill-journeys")]);
                            log::warn!("bill-journey failed ({name}): {e}");
                        }
                    }
                }
            }
            metrics.flush().await;
            log::info!("bill-journeys: {total} done so far");
        }
        log::info!("bill-journeys complete: {total} summaries written");
    }

    async fn run_bill_speakers(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_bill_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            for chunk in pending.chunks(self.concurrency) {
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
                        let prompt =
                            prompts::member_contribution_prompt(&ctx, &p.contributions_text);
                        async move {
                            (
                                p.bill_mention_id,
                                p.speaker_id,
                                summarizer.summarize(&prompt).await,
                            )
                        }
                    })
                    .collect();
                for (mention_id, speaker_id, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_bill_mention_summary(mention_id, speaker_id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "bill-speakers"), ("model", &self.model)],
                            );
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "bill-speakers")]);
                            log::warn!("bill-speaker summary failed: {e}");
                        }
                    }
                }
            }
            metrics.flush().await;
            log::info!("bill-speakers: {total} done so far");
        }
        log::info!("bill-speakers complete: {total} summaries written");
    }

    async fn run_topics(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_topic_appearance_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            for chunk in pending.chunks(self.concurrency) {
                let tasks: Vec<_> = chunk
                    .iter()
                    .map(|p| async move {
                        let prompt = prompts::topic_appearance_prompt(p);
                        (p.topic_id, &p.title, summarizer.summarize(&prompt).await)
                    })
                    .collect();
                for (id, title, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_topic_appearance_summary(id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "topics"), ("model", &self.model)],
                            );
                            log::info!("topic done: {title}");
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "topics")]);
                            log::warn!("topic failed ({title}): {e}");
                        }
                    }
                }
            }
            metrics.flush().await;
            log::info!("topics: {total} done so far");
        }
        log::info!("topics complete: {total} summaries written");
    }

    async fn run_topic_speakers(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_topic_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            for chunk in pending.chunks(self.concurrency) {
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
                        let prompt =
                            prompts::member_contribution_prompt(&ctx, &p.contributions_text);
                        async move {
                            (
                                p.topic_id,
                                p.speaker_id,
                                summarizer.summarize(&prompt).await,
                            )
                        }
                    })
                    .collect();
                for (topic_id, speaker_id, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_topic_summary(topic_id, speaker_id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "topic-speakers"), ("model", &self.model)],
                            );
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "topic-speakers")]);
                            log::warn!("topic-speaker summary failed: {e}");
                        }
                    }
                }
            }
            metrics.flush().await;
            log::info!("topic-speakers: {total} done so far");
        }
        log::info!("topic-speakers complete: {total} summaries written");
    }

    async fn run_sittings(
        &self,
        store: &PostgresStore,
        summarizer: &dyn Summarizer,
        metrics: &dyn MetricsSink,
    ) {
        let mut total = 0u64;
        loop {
            let pending = store
                .pending_sitting_summaries(self.batch)
                .await
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_default();
            if pending.is_empty() {
                break;
            }
            let eff = self.concurrency.min(2);
            for chunk in pending.chunks(eff) {
                let tasks: Vec<_> = chunk
                    .iter()
                    .map(|p| async move {
                        let prompt = prompts::sitting_prompt(p);
                        (
                            p.sitting_id,
                            p.date,
                            &p.house,
                            summarizer.summarize(&prompt).await,
                        )
                    })
                    .collect();
                for (id, date, house, result) in future::join_all(tasks).await {
                    match result {
                        Ok(s) => {
                            store
                                .store_sitting_generated_summary(id, &s, &self.model)
                                .await
                                .ok();
                            total += 1;
                            metrics.counter(
                                "summaries_written",
                                1,
                                &[("target", "sittings"), ("model", &self.model)],
                            );
                            log::info!("sitting done: {date} {house}");
                        }
                        Err(e) => {
                            metrics.counter("summary_failures", 1, &[("target", "sittings")]);
                            log::warn!("sitting failed ({date} {house}): {e}");
                        }
                    }
                }
            }
            metrics.flush().await;
            log::info!("sittings: {total} done so far");
        }
        log::info!("sittings complete: {total} summaries written");
    }
}

#[derive(ValueEnum, Clone)]
enum EnrichTarget {
    BillMentions,
    BillJourneys,
    BillSpeakers,
    Topics,
    TopicSpeakers,
    Sittings,
}

impl Display for EnrichTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BillMentions => write!(f, "bill-mentions"),
            Self::BillJourneys => write!(f, "bill-journeys"),
            Self::BillSpeakers => write!(f, "bill-speakers"),
            Self::Topics => write!(f, "topics"),
            Self::TopicSpeakers => write!(f, "topic-speakers"),
            Self::Sittings => write!(f, "sittings"),
        }
    }
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

    let scraper = HansardScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create scraper: {e}");
        process::exit(1);
    });

    match cli.command {
        Command::Ingest(cmd) => {
            let pipeline = match &cli.metrics_url {
                Some(url) => {
                    log::info!("Metrics: pushing to {url}");
                    IngestPipeline::new(scraper, store)
                        .with_metrics(PrometheusPushSink::new(url, "odnelazm-pipeline"))
                }
                None => IngestPipeline::new(scraper, store).with_metrics(NoopSink),
            };
            cmd.run(&pipeline).await;
        }
        Command::Enrich(cmd) => {
            let llm = LmStudioSummarizer::new(&cmd.llm_url, &cmd.model, cmd.temperature);
            let pipeline = match &cli.metrics_url {
                Some(url) => {
                    log::info!("Metrics: pushing to {url}");
                    IngestPipeline::new(scraper, store)
                        .with_summarizer(llm)
                        .with_metrics(PrometheusPushSink::new(url, "odnelazm-pipeline"))
                }
                None => IngestPipeline::new(scraper, store)
                    .with_summarizer(llm)
                    .with_metrics(NoopSink),
            };
            cmd.run(&pipeline).await;
        }
    }
}
