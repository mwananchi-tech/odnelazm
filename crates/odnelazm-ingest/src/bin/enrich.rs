use futures::future;
use odnelazm_ingest::{
    DataStore,
    enricher::{LmStudioSummarizer, prompts},
    postgres::PostgresStore,
};
use std::process;

#[tokio::main]
async fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://odnelazm:odnelazm@localhost:5432/odnelazm".to_string());

    let llm_url =
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:1234".to_string());

    let llm_model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "google/gemma-4-e4b".to_string());

    let temperature: f32 = std::env::var("LLM_TEMPERATURE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.3);

    let batch: u32 = std::env::var("BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let concurrency: usize = std::env::var("CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let target = std::env::var("ENRICHMENT_TARGET").unwrap_or_else(|_| "bill-mentions".to_string());

    log::info!("Connecting to database…");
    let store = PostgresStore::connect(&database_url)
        .await
        .unwrap_or_else(|e| {
            log::error!("DB connect failed: {e}");
            process::exit(1);
        });

    log::info!("Running migrations…");
    store.migrate().await.unwrap_or_else(|e| {
        log::error!("Migration failed: {e}");
        process::exit(1);
    });

    let llm = LmStudioSummarizer::new(&llm_url, &llm_model, temperature);

    log::info!("Target={target} batch={batch} concurrency={concurrency} model={llm_model}");

    match target.as_str() {
        "bill-speakers" => run_bill_speakers(&store, &llm, batch, concurrency, &llm_model).await,
        "topic-speakers" => run_topic_speakers(&store, &llm, batch, concurrency, &llm_model).await,
        "bill-mentions" => run_bill_mentions(&store, &llm, batch, concurrency, &llm_model).await,
        "bill-journeys" => run_bill_journeys(&store, &llm, batch, concurrency, &llm_model).await,
        "sittings" => run_sittings(&store, &llm, batch, concurrency, &llm_model).await,
        other => {
            log::error!(
                "Unknown ENRICHMENT_TARGET '{other}'. Valid: bill-speakers, topic-speakers, bill-mentions, bill-journeys, sittings"
            );
            process::exit(1);
        }
    }
}

// ── bill-speakers ─────────────────────────────────────────────────────────────

async fn run_bill_speakers(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    llm_model: &str,
) {
    use odnelazm_ingest::summarize::{Summarizer, SummaryContext};
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
                            .store_bill_mention_summary(mention_id, speaker_id, &s, &llm_model)
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

// ── topic-speakers ────────────────────────────────────────────────────────────

async fn run_topic_speakers(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    llm_model: &str,
) {
    use odnelazm_ingest::summarize::{Summarizer, SummaryContext};
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
                            .store_topic_summary(topic_id, speaker_id, &s, &llm_model)
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

// ── bill-mentions ─────────────────────────────────────────────────────────────

async fn run_bill_mentions(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    llm_model: &str,
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
                        store.store_bill_node_summary(id, &s, &llm_model).await.ok();
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

// ── bill-journeys ─────────────────────────────────────────────────────────────

async fn run_bill_journeys(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    llm_model: &str,
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

        // Journey summaries are large — run sequentially within each chunk
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
                        store
                            .store_bill_journey_summary(id, &s, &llm_model)
                            .await
                            .ok();
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

// ── sittings ──────────────────────────────────────────────────────────────────

async fn run_sittings(
    store: &PostgresStore,
    llm: &LmStudioSummarizer,
    batch: u32,
    concurrency: usize,
    llm_model: &str,
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

        // Sitting transcripts are very large — use concurrency of 1-2
        let eff_concurrency = concurrency.min(2);
        for chunk in pending.chunks(eff_concurrency) {
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
                            .store_sitting_generated_summary(id, &s, &llm_model)
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
