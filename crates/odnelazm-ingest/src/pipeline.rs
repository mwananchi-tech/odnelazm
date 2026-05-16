use std::sync::Arc;

use odnelazm::{HansardScraper, HansardSitting, SittingListOptions};

use crate::{
    Result,
    embed::{Embedder, sitting_text},
    extract::{extract_bills, extract_speakers, extract_topics},
    store::{BillMentionRecord, DataStore, MemberEnrichment, MemberRecord, TopicRecord},
    summarize::{Summarizer, SummaryContext, build_prompt},
};

/// Orchestrates scraping → extraction → storage for a stream of sittings.
///
/// `S` is any [`DataStore`] implementation. An optional [`Embedder`] can be
/// attached with [`IngestPipeline::with_embedder`]; if none is provided the
/// embedding step is silently skipped.
pub struct IngestPipeline<S: DataStore> {
    scraper: HansardScraper,
    store: S,
    embedder: Option<Arc<dyn Embedder>>,
    pub summarizer: Option<Arc<dyn Summarizer>>,
}

impl<S: DataStore> IngestPipeline<S> {
    pub fn new(scraper: HansardScraper, store: S) -> Self {
        Self {
            scraper,
            store,
            embedder: None,
            summarizer: None,
        }
    }

    pub fn with_embedder(mut self, embedder: impl Embedder + 'static) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn with_summarizer(mut self, summarizer: impl Summarizer + 'static) -> Self {
        self.summarizer = Some(Arc::new(summarizer));
        self
    }

    pub fn store(&self) -> &S {
        &self.store
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
                        &contributor.contributions_text,
                    )
                    .await?;
            }
        }
        stats.bills_found = mentions.len() as u32;

        // 4. Extract and store topics (questions, statements, motions), statements, and other topics
        let extracted_topics = extract_topics(&sitting);
        for topic in &extracted_topics {
            let topic_id = self
                .store
                .upsert_topic(&TopicRecord {
                    sitting_id,
                    section_type: topic.section_type.clone(),
                    title: topic.title.clone(),
                    speech_count: topic.speech_count,
                })
                .await?;

            for contributor in &topic.contributors {
                let speaker_id = self.store.upsert_speaker(&contributor.speaker).await?;
                self.store
                    .link_speaker_to_topic(
                        topic_id,
                        speaker_id,
                        contributor.speech_count,
                        &contributor.contributions_text,
                    )
                    .await?;
            }
        }

        stats.topics_found = extracted_topics.len() as u32;

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
                            total.topics_found += stats.topics_found;
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
                            total.topics_found += stats.topics_found;
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

    /// Fetch all members for a parliament session, store them, then link them
    /// to existing speaker rows via URL match and fuzzy name match.
    /// Returns the number of speaker→member links created.
    /// Generate and store AI summaries for pending (member, bill) and (member, topic) pairs.
    ///
    /// Requires a [`Summarizer`] attached via [`IngestPipeline::with_summarizer`].
    /// Safe to call incrementally; only rows with `contributions_text IS NOT NULL
    /// AND summary IS NULL` are processed.
    ///
    /// Returns `(bill_summaries, topic_summaries)` generated in this run.
    pub async fn enrich_summaries(&self, batch_size: u32) -> Result<(u64, u64)> {
        let Some(summarizer) = &self.summarizer else {
            log::warn!("enrich_summaries called but no Summarizer is configured");
            return Ok((0, 0));
        };

        let mut bill_count = 0u64;
        let pending_bills = self.store.pending_bill_summaries(batch_size).await?;
        log::info!("Summarising {} bill contributions...", pending_bills.len());
        for p in &pending_bills {
            let ctx = SummaryContext {
                member_name: p
                    .member_name
                    .clone()
                    .unwrap_or_else(|| "Unknown member".into()),
                title: p.bill_name.clone(),
                item_type: "bill".into(),
                stage: p.stage.clone(),
                date: p.date,
                house: p.house.clone(),
            };
            let prompt = build_prompt(&ctx, &p.contributions_text);
            match summarizer.summarize(&prompt).await {
                Ok(summary) => {
                    self.store
                        .store_bill_mention_summary(
                            p.bill_mention_id,
                            p.speaker_id,
                            &summary,
                            "unknown",
                        )
                        .await?;
                    bill_count += 1;
                }
                Err(e) => log::warn!(
                    "Bill summary failed ({} / {}): {e}",
                    p.bill_name,
                    p.member_name.as_deref().unwrap_or("?")
                ),
            }
        }

        let mut topic_count = 0u64;
        let pending_topics = self.store.pending_topic_summaries(batch_size).await?;
        log::info!(
            "Summarising {} topic contributions...",
            pending_topics.len()
        );
        for p in &pending_topics {
            let ctx = SummaryContext {
                member_name: p
                    .member_name
                    .clone()
                    .unwrap_or_else(|| "Unknown member".into()),
                title: p.topic_title.clone(),
                item_type: "topic".into(),
                stage: None,
                date: p.date,
                house: p.house.clone(),
            };
            let prompt = build_prompt(&ctx, &p.contributions_text);
            match summarizer.summarize(&prompt).await {
                Ok(summary) => {
                    self.store
                        .store_topic_summary(p.topic_id, p.speaker_id, &summary, "unknown")
                        .await?;
                    topic_count += 1;
                }
                Err(e) => log::warn!(
                    "Topic summary failed ({} / {}): {e}",
                    p.topic_title,
                    p.member_name.as_deref().unwrap_or("?")
                ),
            }
        }

        Ok((bill_count, topic_count))
    }

    // XXX: limited to 2013-current (mzalendo.com)
    pub async fn import_members(&self, parliament: &str) -> Result<u64> {
        let members = self.scraper.list_all_members_all_houses(parliament).await?;
        log::info!("Importing {} members for {parliament}...", members.len());

        for member in &members {
            self.store
                .upsert_member(&MemberRecord {
                    name: member.name.clone(),
                    url: normalise_url(&member.url),
                    house: member.house.to_string(),
                    parliament: parliament.to_string(),
                    role: member.role.clone(),
                    constituency: member.constituency.clone(),
                })
                .await?;
        }

        log::info!("Members stored — running speaker linkage...");
        let linked = self.store.link_speakers_to_members().await?;
        log::info!("{linked} speaker rows linked to members");
        Ok(linked)
    }

    /// Fetch individual profile pages for all stored members and enrich the DB
    /// with photo, biography, party, committees, and speech statistics.
    /// Safe to re-run since it uses COALESCE so existing values are not overwritten.
    pub async fn enrich_member_profiles(&self, concurrency: usize) -> Result<u64> {
        let members = self.store.list_member_urls().await?;
        log::info!("Enriching {} member profiles...", members.len());
        let mut enriched = 0u64;

        for chunk in members.chunks(concurrency) {
            let fetches: Vec<_> = chunk
                .iter()
                .map(|(id, url)| async move {
                    let result = self
                        .scraper
                        .get_member_profile(&normalise_url(url), false, false)
                        .await;
                    (*id, result)
                })
                .collect();

            for (member_id, result) in futures::future::join_all(fetches).await {
                match result {
                    Ok(profile) => {
                        let e = MemberEnrichment {
                            photo_url: profile.photo_url.map(|p| {
                                if p.starts_with("http") {
                                    p
                                } else {
                                    format!("https://mzalendo.com{p}")
                                }
                            }),
                            biography: profile.biography,
                            party: profile.party,
                            positions: profile.positions,
                            committees: profile.committees,
                            speeches_last_year: profile.speeches_last_year,
                            speeches_total: profile.speeches_total,
                            bills_total: profile.bills_total,
                        };
                        match self.store.enrich_member(member_id, &e).await {
                            Ok(()) => enriched += 1,
                            Err(e) => log::warn!("Enrichment store failed for {member_id}: {e}"),
                        }
                    }
                    Err(e) => log::warn!("Profile fetch failed for {member_id}: {e}"),
                }
            }
        }

        Ok(enriched)
    }
}

#[derive(Debug, Default)]
pub struct IngestStats {
    pub ingested: u32,
    pub skipped: u32,
    pub failed: u32,
    pub bills_found: u32,
    pub topics_found: u32,
    pub speakers_found: u32,
}

impl std::fmt::Display for IngestStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ingested={} skipped={} failed={} bills={} topics={} speakers={}",
            self.ingested,
            self.skipped,
            self.failed,
            self.bills_found,
            self.topics_found,
            self.speakers_found
        )
    }
}

fn normalise_url(url: &str) -> String {
    let u = url.trim();
    if u.ends_with('/') {
        u.to_string()
    } else {
        format!("{u}/")
    }
}
