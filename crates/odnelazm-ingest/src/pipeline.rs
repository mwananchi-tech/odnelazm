use std::sync::Arc;

use odnelazm::{HansardScraper, HansardSitting, SittingListOptions};

use crate::{
    Result,
    embed::{Embedder, sitting_text},
    extract::{extract_bills, extract_speakers},
    store::{BillMentionRecord, DataStore},
};

#[derive(Debug, Default)]
pub struct IngestStats {
    pub ingested: u32,
    pub skipped: u32,
    pub failed: u32,
    pub bills_found: u32,
    pub speakers_found: u32,
}

impl std::fmt::Display for IngestStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ingested={} skipped={} failed={} bills={} speakers={}",
            self.ingested, self.skipped, self.failed, self.bills_found, self.speakers_found
        )
    }
}

/// Orchestrates scraping → extraction → storage for a stream of sittings.
///
/// `S` is any [`DataStore`] implementation. An optional [`Embedder`] can be
/// attached with [`IngestPipeline::with_embedder`]; if none is provided the
/// embedding step is silently skipped.
pub struct IngestPipeline<S: DataStore> {
    scraper: HansardScraper,
    store: S,
    embedder: Option<Arc<dyn Embedder>>,
}

impl<S: DataStore> IngestPipeline<S> {
    pub fn new(scraper: HansardScraper, store: S) -> Self {
        Self {
            scraper,
            store,
            embedder: None,
        }
    }

    pub fn with_embedder(mut self, embedder: impl Embedder + 'static) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    /// Ingest a single fully-fetched sitting. This is the core unit of work;
    /// all other ingest methods funnel through here.
    pub async fn ingest_sitting(&self, sitting: HansardSitting) -> Result<IngestStats> {
        let mut stats = IngestStats::default();

        let sitting_id = self.store.upsert_sitting(&sitting).await?;

        // Extract, store speakers and link speakers to sitting
        let speakers = extract_speakers(&sitting);
        for (speaker, speech_count) in &speakers {
            let speaker_id = self.store.upsert_speaker(speaker).await?;
            self.store
                .link_speaker_to_sitting(speaker_id, sitting_id, *speech_count)
                .await?;
        }
        stats.speakers_found = speakers.len() as u32;

        // Extract and store bill mentions + per-bill contributors
        let mentions = extract_bills(&sitting);
        for mention in &mentions {
            let bill_id = self.store.upsert_bill(&mention.bill).await?;
            let bill_mention_id = self
                .store
                .upsert_bill_mention(
                    bill_id,
                    &BillMentionRecord {
                        sitting_id,
                        house: sitting.house.to_string(),
                        date: sitting.date,
                        stage: mention.stage.clone(),
                        section_title: mention.section_title.clone(),
                        speech_count: mention.speech_count,
                    },
                )
                .await?;

            for contributor in &mention.contributors {
                let speaker_id = self
                    .store
                    .upsert_speaker(&crate::store::SpeakerRecord {
                        name: contributor.name.clone(),
                        url: contributor.url.clone(),
                    })
                    .await?;
                self.store
                    .link_speaker_to_bill_mention(
                        bill_mention_id,
                        speaker_id,
                        contributor.speech_count,
                    )
                    .await?;
            }
        }
        stats.bills_found = mentions.len() as u32;

        // Generate and store embedding (if embedder is configured)
        if let Some(embedder) = &self.embedder {
            let text = sitting_text(&sitting);
            let embedding = embedder.embed(&text).await?;
            self.store
                .store_sitting_embedding(sitting_id, embedding)
                .await?;
        }

        stats.ingested = 1;
        Ok(stats)
    }

    /// Fetch all current-source sittings, skip those already ingested, and
    /// process the rest. Sittings are fetched and ingested in batches of
    /// `concurrency` at a time to avoid hammering the source.
    pub async fn ingest_all(&self, concurrency: usize) -> Result<IngestStats> {
        let listings = self
            .scraper
            .list_sittings(SittingListOptions {
                all: true,
                ..Default::default()
            })
            .await?;

        let ingested_urls: std::collections::HashSet<String> =
            self.store.list_ingested_urls().await?.into_iter().collect();

        let new_listings: Vec<_> = listings
            .into_iter()
            .filter(|l| !ingested_urls.contains(&l.url))
            .collect();

        log::info!(
            "{} sittings total — {} already ingested — {} to process",
            ingested_urls.len() + new_listings.len(),
            ingested_urls.len(),
            new_listings.len(),
        );

        let mut total = IngestStats {
            skipped: ingested_urls.len() as u32,
            ..Default::default()
        };

        for chunk in new_listings.chunks(concurrency) {
            let fetches: Vec<_> = chunk
                .iter()
                .map(|listing| self.scraper.get_sitting(&listing.url))
                .collect();

            let results = futures::future::join_all(fetches).await;

            for (listing, result) in chunk.iter().zip(results) {
                match result {
                    Ok(sitting) => match self.ingest_sitting(sitting).await {
                        Ok(stats) => {
                            total.ingested += stats.ingested;
                            total.bills_found += stats.bills_found;
                            total.speakers_found += stats.speakers_found;
                            log::info!("✓ {}", listing.url);
                        }
                        Err(e) => {
                            log::warn!("Ingest failed for {}: {e}", listing.url);
                            total.failed += 1;
                        }
                    },
                    Err(e) => {
                        log::warn!("Fetch failed for {}: {e}", listing.url);
                        total.failed += 1;
                    }
                }
            }
        }

        Ok(total)
    }

    /// Ingest sittings within a specific date range, skipping already-stored ones.
    pub async fn ingest_range(
        &self,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
        concurrency: usize,
    ) -> Result<IngestStats> {
        let listings = self
            .scraper
            .list_sittings(SittingListOptions {
                start_date: Some(start),
                end_date: Some(end),
                all: true,
                ..Default::default()
            })
            .await?;

        let ingested_urls: std::collections::HashSet<String> =
            self.store.list_ingested_urls().await?.into_iter().collect();

        let new_listings: Vec<_> = listings
            .into_iter()
            .filter(|l| !ingested_urls.contains(&l.url))
            .collect();

        log::info!(
            "Date range {start}–{end}: {} new sittings to ingest",
            new_listings.len()
        );

        let mut total = IngestStats {
            skipped: ingested_urls.len() as u32,
            ..Default::default()
        };

        for chunk in new_listings.chunks(concurrency) {
            let fetches: Vec<_> = chunk
                .iter()
                .map(|listing| self.scraper.get_sitting(&listing.url))
                .collect();

            let results = futures::future::join_all(fetches).await;

            for (listing, result) in chunk.iter().zip(results) {
                match result {
                    Ok(sitting) => match self.ingest_sitting(sitting).await {
                        Ok(stats) => {
                            total.ingested += stats.ingested;
                            total.bills_found += stats.bills_found;
                            total.speakers_found += stats.speakers_found;
                            log::info!("✓ {}", listing.url);
                        }
                        Err(e) => {
                            log::warn!("Ingest failed for {}: {e}", listing.url);
                            total.failed += 1;
                        }
                    },
                    Err(e) => {
                        log::warn!("Fetch failed for {}: {e}", listing.url);
                        total.failed += 1;
                    }
                }
            }
        }

        Ok(total)
    }
}
