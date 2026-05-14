use chrono::NaiveDate;
use odnelazm::HansardScraper;
use odnelazm_ingest::{DataStore, IngestPipeline, PostgresStore};

#[tokio::main]
async fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://odnelazm:odnelazm@localhost:5432/odnelazm".to_string());

    let start_date = std::env::var("START_DATE")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

    let end_date = std::env::var("END_DATE")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

    let concurrency: usize = std::env::var("CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let parliament = std::env::var("PARLIAMENT").unwrap_or_else(|_| "13th-parliament".to_string());

    let skip_sittings = std::env::var("SKIP_SITTINGS").is_ok();
    let skip_members = std::env::var("SKIP_MEMBERS").is_ok();
    let enrich_members = std::env::var("ENRICH_MEMBERS").is_ok();

    log::info!("Connecting to database...");
    let store = PostgresStore::connect(&database_url)
        .await
        .unwrap_or_else(|e| {
            log::error!("Failed to connect: {e}");
            std::process::exit(1);
        });

    log::info!("Running migrations...");
    store.migrate().await.unwrap_or_else(|e| {
        log::error!("Migration failed: {e}");
        std::process::exit(1);
    });

    let scraper = HansardScraper::new().unwrap_or_else(|e| {
        log::error!("Failed to create scraper: {e}");
        std::process::exit(1);
    });

    let pipeline = IngestPipeline::new(scraper, store);

    // ── Sittings ──────────────────────────────────────────────────────────────

    if skip_sittings {
        log::info!("Sittings ingest skipped (SKIP_SITTINGS)");
    }

    if !skip_sittings {
        let stats = match (start_date, end_date) {
            (Some(start), Some(end)) => {
                log::info!("Ingesting range {start} – {end} (concurrency={concurrency})");
                pipeline.ingest_range(start, end, concurrency).await
            }
            _ => {
                log::info!("Ingesting all current sittings (concurrency={concurrency})");
                pipeline.ingest_all(concurrency).await
            }
        }
        .unwrap_or_else(|e| {
            log::error!("Pipeline error: {e}");
            std::process::exit(1);
        });
        log::info!("Sittings — {stats}");
    }

    // ── Members + speaker linkage ─────────────────────────────────────────────

    let enrich_batch: u32 = std::env::var("ENRICH_BATCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if !skip_members {
        let linked = pipeline
            .import_members(&parliament)
            .await
            .unwrap_or_else(|e| {
                log::error!("Member import error: {e}");
                std::process::exit(1);
            });
        log::info!("Members done — {linked} speaker→member links created");
    }

    if enrich_members {
        let enriched = pipeline
            .enrich_member_profiles(concurrency)
            .await
            .unwrap_or_else(|e| {
                log::error!("Member enrichment error: {e}");
                std::process::exit(1);
            });
        log::info!("Member profiles enriched — {enriched} updated");
    }

    // ── AI enrichment (optional, requires ENRICH_BATCH > 0 + a Summarizer) ──

    if enrich_batch > 0 {
        let (bills, topics) = pipeline
            .enrich_summaries(enrich_batch)
            .await
            .unwrap_or_else(|e| {
                log::error!("Enrichment error: {e}");
                std::process::exit(1);
            });
        log::info!("Enrichment done — {bills} bill summaries, {topics} topic summaries generated");
    }
}
