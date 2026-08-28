use std::sync::Arc;

use odnelazm::{HansardListing, HansardScraper, HansardSitting, SittingListOptions};

use crate::{
    Result,
    embed::{Embedder, sitting_text},
    extract::{extract_bills, extract_speakers, extract_topics},
    metrics::MetricsSink,
    store::{BillMentionRecord, DataStore, MemberRecord, SittingSourceIdentity, TopicRecord},
    summarize::Summarizer,
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
    pub metrics: Option<Arc<dyn MetricsSink>>,
}

impl<S: DataStore> IngestPipeline<S> {
    pub fn new(scraper: HansardScraper, store: S) -> Self {
        Self {
            scraper,
            store,
            embedder: None,
            summarizer: None,
            metrics: None,
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

    pub fn with_metrics(mut self, sink: impl MetricsSink + 'static) -> Self {
        self.metrics = Some(Arc::new(sink));
        self
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// Ingest a single fully-fetched sitting. This is the core unit of work;
    /// all other ingest methods funnel through here.
    async fn ingest_sitting(&self, sitting: HansardSitting) -> Result<IngestStats> {
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
    pub async fn ingest_all_sittings(&self, concurrency: usize) -> Result<IngestStats> {
        let listings = self
            .scraper
            .list_sittings(SittingListOptions {
                all: true,
                ..Default::default()
            })
            .await?;

        let ingested_sources = self.store.list_ingested_sitting_sources().await?;
        let total_listings = listings.len();
        let new_listings = filter_ingested_listings(listings, &ingested_sources);
        let skipped = total_listings - new_listings.len();

        log::info!(
            "{} sittings total, {} already ingested, {} to process",
            total_listings,
            skipped,
            new_listings.len(),
        );

        let mut total = IngestStats {
            skipped: skipped as u32,
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
    pub async fn ingest_sittings_in_range(
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

        let ingested_sources = self.store.list_ingested_sitting_sources().await?;
        let total_listings = listings.len();
        let new_listings = filter_ingested_listings(listings, &ingested_sources);
        let skipped = total_listings - new_listings.len();

        log::info!(
            "Date range {start}–{end}: {} new sittings to ingest",
            new_listings.len()
        );

        let mut total = IngestStats {
            skipped: skipped as u32,
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

    // XXX: limited to 2013-current (mzalendo.com)
    pub async fn ingest_members(&self, parliament: &str) -> Result<u64> {
        let members = self.scraper.list_all_members_all_houses(parliament).await?;
        log::info!("Importing {} members for {parliament}...", members.len());

        for member in &members {
            self.store
                .upsert_member(&MemberRecord {
                    name: member.name.clone(),
                    url: normalise_url(&member.url),
                    source: odnelazm::DataSource::Current,
                    house: member.house.to_string(),
                    parliament: parliament.to_string(),
                    role: member.role.clone(),
                    constituency: member.constituency.clone(),
                })
                .await?;
        }

        log::info!("Members stored, running speaker linkage...");
        let linked = self.store.link_speakers_to_members(parliament).await?;
        log::info!("{linked} speaker rows linked to members");

        let bill_sponsors = self.store.link_bill_sponsors_to_members().await?;
        log::info!("{bill_sponsors} bill sponsor rows linked to members");

        Ok(linked)
    }

    /// Fetch individual profile pages for all stored members and enrich the DB
    /// with photo, biography, party, committees, and speech statistics.
    /// Safe to re-run since it uses COALESCE so existing values are not overwritten.
    pub async fn ingest_member_profiles(&self, concurrency: usize) -> Result<u64> {
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
                        match self
                            .store
                            .update_member_profile(member_id, &profile.into())
                            .await
                        {
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

fn filter_ingested_listings(
    listings: Vec<HansardListing>,
    ingested: &[SittingSourceIdentity],
) -> Vec<HansardListing> {
    let external_keys: std::collections::HashSet<_> = ingested
        .iter()
        .filter_map(|identity| {
            identity
                .external_key
                .as_ref()
                .map(|key| (&identity.source_key, key))
        })
        .collect();
    let normalized_urls: std::collections::HashSet<_> = ingested
        .iter()
        .map(|identity| (&identity.source_key, &identity.normalized_url))
        .collect();

    listings
        .into_iter()
        .filter(|listing| {
            let identity = SittingSourceIdentity::from_observation(listing.source, &listing.url);
            let external_match = identity
                .external_key
                .as_ref()
                .is_some_and(|key| external_keys.contains(&(&identity.source_key, key)));
            !external_match
                && !normalized_urls.contains(&(&identity.source_key, &identity.normalized_url))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use odnelazm::{DataSource, HansardListing, House};

    use super::{SittingSourceIdentity, filter_ingested_listings};

    fn listing(source: DataSource, url: &str) -> HansardListing {
        HansardListing {
            house: House::NationalAssembly,
            date: NaiveDate::from_ymd_opt(2026, 2, 12).unwrap(),
            url: url.to_owned(),
            title: "Sitting".to_owned(),
            session_type: Some("Afternoon Sitting".to_owned()),
            start_time: None,
            end_time: None,
            source,
        }
    }

    #[test]
    fn skip_logic_matches_current_alias_by_external_key_before_url() {
        let ingested = [SittingSourceIdentity::from_observation(
            DataSource::Current,
            "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/",
        )];
        let listings = vec![
            listing(
                DataSource::Current,
                "/democracy-tools/hansard/document/2438/",
            ),
            listing(
                DataSource::Current,
                "/democracy-tools/hansard/document/3096/",
            ),
        ];

        let remaining = filter_ingested_listings(listings, &ingested);
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].url.contains("3096"));
    }

    #[test]
    fn skip_logic_keeps_source_namespaces_separate() {
        let ingested = [SittingSourceIdentity::from_observation(
            DataSource::Archive,
            "https://info.mzalendo.com/hansard/sitting/senate/2020-12-29-14-30-00",
        )];
        let listings = vec![listing(
            DataSource::Current,
            "/democracy-tools/hansard/document/00/",
        )];

        assert_eq!(filter_ingested_listings(listings, &ingested).len(), 1);
    }
}
