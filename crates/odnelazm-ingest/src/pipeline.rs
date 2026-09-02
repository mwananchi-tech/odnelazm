use std::sync::Arc;

use odnelazm::{
    DataSource, HansardListing, HansardListingKind, HansardScraper, HansardSitting,
    SittingListOptions,
};

use crate::{
    Result,
    embed::{Embedder, sitting_text},
    extract::{extract_bills, extract_speakers, extract_topics},
    metrics::MetricsSink,
    store::{
        DataStore, IngestionRunCompletion, IngestionRunStatus, MemberRecord, SittingSourceIdentity,
    },
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
    dry_run: bool,
}

impl<S: DataStore> IngestPipeline<S> {
    pub fn new(scraper: HansardScraper, store: S) -> Self {
        Self {
            scraper,
            store,
            embedder: None,
            summarizer: None,
            metrics: None,
            dry_run: false,
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

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// Ingest a single fully-fetched sitting. This is the core unit of work;
    /// all other ingest methods funnel through here.
    async fn ingest_sitting(
        &self,
        sitting: HansardSitting,
        parliament: &str,
    ) -> Result<IngestStats> {
        let mut stats = IngestStats::default();

        let speakers = extract_speakers(&sitting);
        stats.speakers_found = speakers.len() as u32;

        let mentions = extract_bills(&sitting);
        stats.bills_found = mentions.len() as u32;

        let extracted_topics = extract_topics(&sitting);
        stats.topics_found = extracted_topics.len() as u32;

        if self.dry_run {
            stats.ingested = 1;
            return Ok(stats);
        }

        let reconciliation = self
            .store
            .reconcile_sitting(
                &sitting,
                parliament,
                &speakers,
                &mentions,
                &extracted_topics,
            )
            .await?;
        stats.speakers_linked = reconciliation.speakers_linked as u32;

        // Generate and store embedding (if embedder is configured)
        if let Some(embedder) = &self.embedder {
            let text = sitting_text(&sitting);
            let embedding = embedder.embed(&text).await?;
            self.store
                .store_sitting_embedding(reconciliation.sitting_id, embedding)
                .await?;
        }

        stats.ingested = 1;
        Ok(stats)
    }

    /// Fetch all current-source sittings, skip those already ingested, and
    /// process the rest. Sittings are fetched and ingested in batches of
    /// `concurrency` at a time to avoid hammering the source.
    pub async fn ingest_all_sittings(
        &self,
        parliament: &str,
        concurrency: usize,
    ) -> Result<IngestStats> {
        self.ingest_sittings(
            SittingListOptions {
                all: true,
                ..Default::default()
            },
            parliament,
            concurrency,
        )
        .await
    }

    /// Ingest sittings within a specific date range, skipping already-stored ones.
    pub async fn ingest_sittings_in_range(
        &self,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
        parliament: &str,
        concurrency: usize,
    ) -> Result<IngestStats> {
        self.ingest_sittings(
            SittingListOptions {
                start_date: Some(start),
                end_date: Some(end),
                all: true,
                ..Default::default()
            },
            parliament,
            concurrency,
        )
        .await
    }

    async fn ingest_sittings(
        &self,
        options: SittingListOptions,
        parliament: &str,
        concurrency: usize,
    ) -> Result<IngestStats> {
        let source_keys = sitting_source_keys(&options);
        let mut runs = Vec::new();
        for source_key in source_keys {
            let run_id = self
                .store
                .start_ingestion_run(
                    source_key,
                    serde_json::json!({
                        "operation": "sittings",
                        "dry_run": self.dry_run,
                        "start_date": options.start_date,
                        "end_date": options.end_date,
                    }),
                )
                .await?;
            runs.push((source_key, run_id, RunAccumulator::default()));
        }

        let listings = match self.scraper.list_sittings(options).await {
            Ok(listings) => listings,
            Err(error) => {
                for (_, run_id, _) in &runs {
                    self.store
                        .finish_ingestion_run(*run_id, &failed_before_discovery(&error.to_string()))
                        .await?;
                }
                return Err(error.into());
            }
        };
        let ingested_sources = match self.store.list_ingested_sitting_sources().await {
            Ok(sources) => sources,
            Err(error) => {
                for (_, run_id, _) in &runs {
                    self.store
                        .finish_ingestion_run(*run_id, &failed_before_discovery(&error.to_string()))
                        .await?;
                }
                return Err(error);
            }
        };

        let mut actionable = Vec::new();
        for listing in listings {
            let source_key = source_key(listing.source);
            let accumulator = runs
                .iter_mut()
                .find(|(key, _, _)| *key == source_key)
                .map(|(_, _, accumulator)| accumulator)
                .expect("run created for routed source");
            accumulator.discovered += 1;
            if listing.kind == HansardListingKind::ExternalPdf {
                accumulator.record_unsupported(&listing);
            } else if is_ingested_listing(&listing, &ingested_sources) {
                accumulator.record_already_ingested();
            } else {
                actionable.push(listing);
            }
        }

        let mut total = IngestStats::default();
        for chunk in actionable.chunks(concurrency.max(1)) {
            let fetches = chunk
                .iter()
                .map(|listing| self.scraper.get_sitting(&listing.url));
            for (listing, result) in chunk.iter().zip(futures::future::join_all(fetches).await) {
                let accumulator = runs
                    .iter_mut()
                    .find(|(key, _, _)| *key == source_key(listing.source))
                    .map(|(_, _, accumulator)| accumulator)
                    .expect("run created for routed source");
                match result {
                    Ok(sitting) => match self.ingest_sitting(sitting, parliament).await {
                        Ok(stats) => {
                            accumulator.succeeded += 1;
                            accumulator.speakers_linked += stats.speakers_linked as u64;
                            total.add(&stats);
                            log::info!("ingested {}", listing.url);
                        }
                        Err(error) => accumulator.record_failure(&listing.url, "store", &error),
                    },
                    Err(error) => {
                        accumulator.record_failure(&listing.url, "fetch_or_parse", &error)
                    }
                }
            }
        }

        let mut incomplete = false;
        let mut unsupported = 0;
        for (_, run_id, accumulator) in runs {
            total.skipped += accumulator.skipped as u32;
            total.failed += accumulator.failed as u32;
            unsupported += accumulator.unsupported_count();
            incomplete |= accumulator.is_incomplete();
            self.store
                .finish_ingestion_run(run_id, &accumulator.completion())
                .await?;
        }
        if incomplete {
            return Err(crate::IngestError::Incomplete {
                operation: "sittings".to_owned(),
                details: format!("{} failed, {unsupported} unsupported", total.failed),
            });
        }
        Ok(total)
    }

    // XXX: limited to 2013-current (mzalendo.com)
    pub async fn ingest_members(&self, parliament: &str) -> Result<u64> {
        let run_id = self
            .store
            .start_ingestion_run(
                "mzalendo-current",
                serde_json::json!({
                    "operation": "members", "parliament": parliament, "dry_run": self.dry_run,
                }),
            )
            .await?;
        let members = match self.scraper.list_all_members_all_houses(parliament).await {
            Ok(members) => members,
            Err(error) => {
                self.store
                    .finish_ingestion_run(run_id, &failed_before_discovery(&error.to_string()))
                    .await?;
                return Err(error.into());
            }
        };
        log::info!("Importing {} members for {parliament}...", members.len());
        let mut run = RunAccumulator {
            discovered: members.len() as u64,
            ..Default::default()
        };
        for member in &members {
            let result = if self.dry_run {
                Ok(uuid::Uuid::nil())
            } else {
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
                    .await
            };
            match result {
                Ok(_) => run.succeeded += 1,
                Err(error) => run.record_failure(&member.url, "store", &error),
            }
        }
        let mut linked = 0;
        if !self.dry_run && run.failed == 0 {
            match self.store.link_speakers_to_members(parliament).await {
                Ok(count) => linked = count,
                Err(error) => run.operation_errors.push(error.to_string()),
            }
            if let Err(error) = self.store.link_bill_sponsors_to_members().await {
                run.operation_errors.push(error.to_string());
            }
        }
        let incomplete = run.is_incomplete();
        self.store
            .finish_ingestion_run(run_id, &run.completion())
            .await?;
        if incomplete {
            return Err(crate::IngestError::Incomplete {
                operation: "members".to_owned(),
                details: format!("{} member(s) failed", run.failed),
            });
        }
        Ok(linked)
    }

    /// Fetch individual profile pages for all stored members and enrich the DB
    /// with photo, biography, party, committees, and speech statistics.
    /// Safe to re-run since it uses COALESCE so existing values are not overwritten.
    pub async fn ingest_member_profiles(&self, concurrency: usize) -> Result<u64> {
        let run_id = self
            .store
            .start_ingestion_run(
                "mzalendo-current",
                serde_json::json!({
                    "operation": "member_profiles", "dry_run": self.dry_run,
                }),
            )
            .await?;
        let members = match self.store.list_member_urls().await {
            Ok(members) => members,
            Err(error) => {
                self.store
                    .finish_ingestion_run(run_id, &failed_before_discovery(&error.to_string()))
                    .await?;
                return Err(error);
            }
        };
        log::info!("Enriching {} member profiles...", members.len());
        let mut enriched = 0u64;
        let mut run = RunAccumulator {
            discovered: members.len() as u64,
            ..Default::default()
        };

        for chunk in members.chunks(concurrency.max(1)) {
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
                        let result = if self.dry_run {
                            Ok(())
                        } else {
                            self.store
                                .update_member_profile(member_id, &profile.into())
                                .await
                        };
                        match result {
                            Ok(()) => {
                                enriched += 1;
                                run.succeeded += 1;
                            }
                            Err(error) => {
                                run.record_failure(&member_id.to_string(), "store", &error)
                            }
                        }
                    }
                    Err(error) => {
                        run.record_failure(&member_id.to_string(), "fetch_or_parse", &error)
                    }
                }
            }
        }
        let incomplete = run.is_incomplete();
        self.store
            .finish_ingestion_run(run_id, &run.completion())
            .await?;
        if incomplete {
            return Err(crate::IngestError::Incomplete {
                operation: "member profiles".to_owned(),
                details: format!("{} profile(s) failed", run.failed),
            });
        }
        Ok(enriched)
    }
}

#[derive(Debug, Default)]
struct RunAccumulator {
    discovered: u64,
    succeeded: u64,
    skipped: u64,
    failed: u64,
    already_ingested: u64,
    unsupported: Vec<serde_json::Value>,
    failures: Vec<serde_json::Value>,
    operation_errors: Vec<String>,
    speakers_linked: u64,
}

impl RunAccumulator {
    fn record_already_ingested(&mut self) {
        self.skipped += 1;
        self.already_ingested += 1;
    }

    fn record_unsupported(&mut self, listing: &HansardListing) {
        self.skipped += 1;
        self.unsupported.push(serde_json::json!({
            "reason": "unsupported_external_pdf",
            "url": listing.url,
            "title": listing.title,
        }));
    }

    fn unsupported_count(&self) -> u64 {
        self.unsupported.len() as u64
    }

    fn record_failure(&mut self, item: &str, stage: &str, error: &impl std::fmt::Display) {
        self.failed += 1;
        self.failures.push(serde_json::json!({
            "item": item,
            "stage": stage,
            "error": error.to_string(),
        }));
    }

    fn is_incomplete(&self) -> bool {
        self.failed > 0 || !self.unsupported.is_empty() || !self.operation_errors.is_empty()
    }

    fn completion(&self) -> IngestionRunCompletion {
        let status = if !self.is_incomplete() {
            IngestionRunStatus::Succeeded
        } else if self.discovered > 0 && self.failed == self.discovered {
            IngestionRunStatus::Failed
        } else {
            IngestionRunStatus::Partial
        };
        let error_message = self.is_incomplete().then(|| {
            format!(
                "{} failed, {} unsupported, {} operation errors",
                self.failed,
                self.unsupported.len(),
                self.operation_errors.len()
            )
        });
        IngestionRunCompletion {
            status,
            discovered: self.discovered,
            processed: self.succeeded + self.skipped + self.failed,
            succeeded: self.succeeded,
            skipped: self.skipped,
            failed: self.failed,
            error_message,
            error_metadata: serde_json::json!({
                "already_ingested_count": self.already_ingested,
                "unsupported_count": self.unsupported_count(),
                "unsupported": self.unsupported,
                "failures": self.failures,
                "operation_errors": self.operation_errors,
                "speakers_linked_count": self.speakers_linked,
            }),
        }
    }
}

fn failed_before_discovery(error: &str) -> IngestionRunCompletion {
    IngestionRunCompletion {
        status: IngestionRunStatus::Failed,
        discovered: 0,
        processed: 0,
        succeeded: 0,
        skipped: 0,
        failed: 0,
        error_message: Some(error.to_owned()),
        error_metadata: serde_json::json!({ "failures": [{
            "stage": "discovery",
            "error": error,
        }]}),
    }
}

fn source_key(source: DataSource) -> &'static str {
    match source {
        DataSource::Current => "mzalendo-current",
        DataSource::Archive => "mzalendo-archive",
    }
}

fn sitting_source_keys(options: &SittingListOptions) -> Vec<&'static str> {
    let cutoff = chrono::NaiveDate::from_ymd_opt(2013, 3, 28).expect("valid cutoff");
    match (options.start_date, options.end_date) {
        (_, Some(end)) if end < cutoff => vec!["mzalendo-archive"],
        (Some(start), _) if start >= cutoff => vec!["mzalendo-current"],
        (None, None) => vec!["mzalendo-current"],
        _ => vec!["mzalendo-archive", "mzalendo-current"],
    }
}

fn is_ingested_listing(listing: &HansardListing, ingested: &[SittingSourceIdentity]) -> bool {
    let identity = SittingSourceIdentity::from_observation(listing.source, &listing.url);
    ingested.iter().any(|stored| {
        stored.source_key == identity.source_key
            && (identity.external_key.is_some() && stored.external_key == identity.external_key
                || stored.normalized_url == identity.normalized_url)
    })
}

#[cfg(test)]
fn filter_ingested_listings(
    listings: Vec<HansardListing>,
    ingested: &[SittingSourceIdentity],
) -> Vec<HansardListing> {
    listings
        .into_iter()
        .filter(|listing| !is_ingested_listing(listing, ingested))
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
    pub speakers_linked: u32,
}

impl IngestStats {
    fn add(&mut self, other: &Self) {
        self.ingested += other.ingested;
        self.skipped += other.skipped;
        self.failed += other.failed;
        self.bills_found += other.bills_found;
        self.topics_found += other.topics_found;
        self.speakers_found += other.speakers_found;
        self.speakers_linked += other.speakers_linked;
    }
}

impl std::fmt::Display for IngestStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ingested={} skipped={} failed={} bills={} topics={} speakers={} speakers_linked={}",
            self.ingested,
            self.skipped,
            self.failed,
            self.bills_found,
            self.topics_found,
            self.speakers_found,
            self.speakers_linked
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
    use odnelazm::{DataSource, HansardListing, HansardListingKind, House};

    use super::{RunAccumulator, SittingSourceIdentity, filter_ingested_listings};

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
            kind: HansardListingKind::Transcript,
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

    #[test]
    fn already_ingested_and_unsupported_outcomes_are_not_conflated() {
        let mut run = RunAccumulator {
            discovered: 910,
            ..Default::default()
        };
        for _ in 0..909 {
            run.record_already_ingested();
        }
        let mut pdf = listing(
            DataSource::Current,
            "https://www.parliament.go.ke/hansard.pdf",
        );
        pdf.kind = HansardListingKind::ExternalPdf;
        run.record_unsupported(&pdf);

        let completion = run.completion();
        assert_eq!(completion.status.as_str(), "partial");
        assert_eq!(completion.processed, 910);
        assert_eq!(completion.skipped, 910);
        assert_eq!(
            completion.error_message.as_deref(),
            Some("0 failed, 1 unsupported, 0 operation errors")
        );
        assert_eq!(completion.error_metadata["already_ingested_count"], 909);
        assert_eq!(completion.error_metadata["unsupported_count"], 1);
        assert!(completion.validate().is_ok());
    }

    #[test]
    fn all_already_ingested_outcomes_succeed() {
        let mut run = RunAccumulator {
            discovered: 909,
            ..Default::default()
        };
        for _ in 0..909 {
            run.record_already_ingested();
        }

        let completion = run.completion();
        assert_eq!(completion.status.as_str(), "succeeded");
        assert_eq!(completion.processed, 909);
        assert_eq!(completion.skipped, 909);
        assert_eq!(completion.succeeded, 0);
        assert_eq!(completion.error_message, None);
        assert_eq!(completion.error_metadata["already_ingested_count"], 909);
        assert_eq!(completion.error_metadata["unsupported_count"], 0);
        assert!(completion.validate().is_ok());
    }
}
