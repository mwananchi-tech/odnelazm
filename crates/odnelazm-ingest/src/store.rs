use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use odnelazm::{DataSource, HansardSitting};

use crate::Result;
use crate::extract::{ExtractedBillMention, ExtractedTopic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionRunStatus {
    Succeeded,
    Partial,
    Failed,
}

impl IngestionRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IngestionRunCompletion {
    pub status: IngestionRunStatus,
    pub discovered: u64,
    pub processed: u64,
    pub succeeded: u64,
    pub skipped: u64,
    pub failed: u64,
    pub error_message: Option<String>,
    pub error_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SittingReconciliation {
    pub sitting_id: Uuid,
    pub speakers_linked: u64,
}

impl IngestionRunCompletion {
    pub fn validate(&self) -> Result<()> {
        let outcomes = self
            .succeeded
            .checked_add(self.skipped)
            .and_then(|count| count.checked_add(self.failed));
        if outcomes != Some(self.processed) || self.processed > self.discovered {
            return Err(crate::IngestError::Store(format!(
                "invalid ingestion run counts: discovered={} processed={} succeeded={} skipped={} failed={}",
                self.discovered, self.processed, self.succeeded, self.skipped, self.failed
            )));
        }
        if self.status == IngestionRunStatus::Succeeded
            && (self.failed != 0 || self.error_message.is_some())
        {
            return Err(crate::IngestError::Store(
                "succeeded ingestion run cannot contain failures".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SittingSourceIdentity {
    pub source_key: String,
    pub source_url: String,
    pub normalized_url: String,
    pub external_key: Option<String>,
}

impl SittingSourceIdentity {
    pub fn from_sitting(sitting: &HansardSitting) -> Self {
        Self::from_observation(sitting.source, &sitting.url)
    }

    pub fn from_observation(source: DataSource, source_url: &str) -> Self {
        let source_key = match source {
            DataSource::Current => "mzalendo-current",
            DataSource::Archive => "mzalendo-archive",
        };
        let base_url = match source {
            DataSource::Current => "https://mzalendo.com",
            DataSource::Archive => "https://info.mzalendo.com",
        };
        let absolute_url =
            if source_url.starts_with("http://") || source_url.starts_with("https://") {
                source_url.to_owned()
            } else {
                format!("{base_url}/{}", source_url.trim_start_matches('/'))
            };
        let normalized_url = normalize_source_url(&absolute_url);
        let external_key = match source {
            DataSource::Current => current_external_key(&normalized_url),
            DataSource::Archive => None,
        };

        Self {
            source_key: source_key.to_owned(),
            source_url: absolute_url,
            normalized_url,
            external_key,
        }
    }
}

fn normalize_source_url(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.trim_end_matches('/').to_owned();
    };
    if matches!(parsed.host_str(), Some("mzalendo.com" | "www.mzalendo.com")) {
        let _ = parsed.set_host(Some("mzalendo.com"));
        let _ = parsed.set_scheme("https");
    } else if matches!(
        parsed.host_str(),
        Some("info.mzalendo.com" | "www.info.mzalendo.com")
    ) {
        let _ = parsed.set_host(Some("info.mzalendo.com"));
        let _ = parsed.set_scheme("https");
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    let path = parsed.path().trim_end_matches('/').to_owned();
    parsed.set_path(if path.is_empty() { "/" } else { &path });
    parsed.to_string().trim_end_matches('/').to_owned()
}

fn current_external_key(normalized_url: &str) -> Option<String> {
    let parsed = url::Url::parse(normalized_url).ok()?;
    let segments: Vec<_> = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect();

    if let ["democracy-tools", "hansard", "document", id] = segments.as_slice()
        && id.chars().all(|c| c.is_ascii_digit())
    {
        return Some((*id).to_owned());
    }

    if let ["democracy-tools", "hansard", slug] = segments.as_slice()
        && let Some((_, id)) = slug.rsplit_once('-')
        && id.chars().all(|c| c.is_ascii_digit())
    {
        return Some(id.to_owned());
    }

    None
}

#[derive(Debug, Clone)]
pub struct SpeakerRecord {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BillRecord {
    pub name: String,
    pub bill_number: Option<String>,
    pub year: Option<i32>,
    pub sponsor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BillMentionRecord {
    pub sitting_id: Uuid,
    pub house: String,
    pub date: NaiveDate,
    pub stage: Option<String>,
    pub section_title: String,
    pub speech_count: u32,
}

#[derive(Debug, Clone)]
pub struct TopicRecord {
    pub sitting_id: Uuid,
    pub section_type: String,
    pub title: String,
    pub speech_count: u32,
}

#[derive(Debug, Clone)]
pub struct MemberRecord {
    pub name: String,
    pub url: String,
    pub source: DataSource,
    pub house: String,
    pub parliament: String,
    pub role: Option<String>,
    pub constituency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSourceIdentity {
    pub source_key: String,
    pub source_url: String,
    pub normalized_url: String,
    pub external_key: Option<String>,
}

impl MemberSourceIdentity {
    pub fn from_member(member: &MemberRecord) -> Self {
        Self::from_observation(member.source, &member.url)
    }

    pub fn from_observation(source: DataSource, source_url: &str) -> Self {
        let (source_key, base_url) = match source {
            DataSource::Current => ("mzalendo-current", "https://mzalendo.com"),
            DataSource::Archive => ("mzalendo-archive", "https://info.mzalendo.com"),
        };
        let source_url = source_url.trim();
        let absolute_url =
            if source_url.starts_with("http://") || source_url.starts_with("https://") {
                source_url.to_owned()
            } else {
                format!("{base_url}/{}", source_url.trim_start_matches('/'))
            };
        let normalized_url = normalize_source_url(&absolute_url);
        let external_key = member_external_key(&normalized_url);

        Self {
            source_key: source_key.to_owned(),
            source_url: absolute_url,
            normalized_url,
            external_key,
        }
    }
}

pub(crate) fn normalize_member_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn member_external_key(normalized_url: &str) -> Option<String> {
    let parsed = url::Url::parse(normalized_url).ok()?;
    let segments: Vec<_> = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect();

    match segments.as_slice() {
        ["person", slug] | ["mps-performance", _, _, slug] if !slug.is_empty() => {
            Some((*slug).to_owned())
        }
        _ => None,
    }
}

/// Profile data fetched from a member's individual profile page.
#[derive(Debug, Clone)]
pub struct MemberProfileData {
    pub photo_url: Option<String>,
    pub biography: Option<String>,
    pub party: Option<String>,
    pub positions: Vec<String>,
    pub committees: Vec<String>,
    pub speeches_last_year: Option<u32>,
    pub speeches_total: Option<u32>,
    pub bills_total: Option<u32>,
}

impl From<odnelazm::MemberProfile> for MemberProfileData {
    fn from(p: odnelazm::MemberProfile) -> Self {
        Self {
            photo_url: p.photo_url.map(|url| {
                if url.starts_with("http") {
                    url
                } else {
                    format!("https://mzalendo.com{url}")
                }
            }),
            biography: p.biography,
            party: p.party,
            positions: p.positions,
            committees: p.committees,
            speeches_last_year: p.speeches_last_year,
            speeches_total: p.speeches_total,
            bills_total: p.bills_total,
        }
    }
}

/// A (bill_mention, speaker) pair that has contribution text but no summary yet.
#[derive(Debug)]
pub struct PendingBillSummary {
    pub bill_mention_id: Uuid,
    pub speaker_id: Uuid,
    pub member_name: Option<String>,
    pub bill_name: String,
    pub date: NaiveDate,
    pub house: String,
    pub stage: Option<String>,
    pub contributions_text: String,
}

/// A (topic, speaker) pair that has contribution text but no summary yet.
#[derive(Debug)]
pub struct PendingTopicSummary {
    pub topic_id: Uuid,
    pub speaker_id: Uuid,
    pub member_name: Option<String>,
    pub topic_title: String,
    pub section_type: String,
    pub date: NaiveDate,
    pub house: String,
    pub contributions_text: String,
}

/// A topic row needing a topic-level summary across all contributors.
/// Carries the full sitting transcript as JSON for context.
#[derive(Debug)]
pub struct PendingTopicAppearanceSummary {
    pub topic_id: Uuid,
    pub title: String,
    pub section_type: String,
    pub date: NaiveDate,
    pub house: String,
    pub session_type: String,
    pub sitting_raw_json: serde_json::Value,
}

/// A bill_mention row needing a node-level summary.
/// Carries the full sitting transcript as JSON for context.
#[derive(Debug)]
pub struct PendingBillAppearanceSummary {
    pub bill_mention_id: Uuid,
    pub bill_name: String,
    pub bill_number: Option<String>,
    pub stage: Option<String>,
    pub section_title: String,
    pub date: NaiveDate,
    pub house: String,
    pub session_type: String,
    pub sitting_raw_json: serde_json::Value,
}

/// One sitting's context for assembling a bill's full journey summary.
#[derive(Debug, serde::Deserialize)]
pub struct BillMentionContext {
    pub date: NaiveDate,
    pub house: String,
    pub stage: Option<String>,
    pub section_title: String,
    pub summary: Option<String>,
    pub speakers_text: Option<String>,
}

/// A bill row needing a full journey summary.
#[derive(Debug)]
pub struct PendingBillJourneySummary {
    pub bill_id: Uuid,
    pub bill_name: String,
    pub bill_number: Option<String>,
    pub year: Option<i32>,
    pub sponsor: Option<String>,
    pub mentions: Vec<BillMentionContext>,
}

/// A sitting row needing a rich AI-generated summary.
#[derive(Debug)]
pub struct PendingSittingSummary {
    pub sitting_id: Uuid,
    pub url: String,
    pub date: NaiveDate,
    pub house: String,
    pub session_type: String,
    pub existing_summary: Option<String>,
    pub raw_json: serde_json::Value,
}

#[async_trait]
pub trait DataStore: Send + Sync {
    async fn migrate(&self) -> Result<()>;

    async fn start_ingestion_run(
        &self,
        source_key: &str,
        metadata: serde_json::Value,
    ) -> Result<Uuid>;
    async fn finish_ingestion_run(
        &self,
        run_id: Uuid,
        completion: &IngestionRunCompletion,
    ) -> Result<()>;

    async fn upsert_sitting(&self, sitting: &HansardSitting) -> Result<Uuid>;
    /// Atomically replace all sitting-scoped derived projections with one
    /// extraction snapshot. Missing rows are retained but marked inactive.
    async fn reconcile_sitting(
        &self,
        sitting: &HansardSitting,
        speakers: &[(SpeakerRecord, u32)],
        bill_mentions: &[ExtractedBillMention],
        topics: &[ExtractedTopic],
    ) -> Result<SittingReconciliation>;
    async fn list_ingested_sitting_sources(&self) -> Result<Vec<SittingSourceIdentity>>;
    async fn store_sitting_embedding(&self, sitting_id: Uuid, embedding: Vec<f32>) -> Result<()>;

    async fn upsert_speaker(&self, speaker: &SpeakerRecord) -> Result<Uuid>;
    async fn link_speaker_to_sitting(
        &self,
        speaker_id: Uuid,
        sitting_id: Uuid,
        speech_count: u32,
    ) -> Result<()>;

    async fn upsert_bill(&self, bill: &BillRecord) -> Result<Uuid>;
    async fn upsert_bill_mention(&self, bill_id: Uuid, mention: &BillMentionRecord)
    -> Result<Uuid>;
    async fn link_speaker_to_bill_mention(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
        contributions_text: &str,
    ) -> Result<()>;

    async fn upsert_topic(&self, topic: &TopicRecord) -> Result<Uuid>;
    async fn link_speaker_to_topic(
        &self,
        topic_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
        contributions_text: &str,
    ) -> Result<()>;

    async fn upsert_member(&self, member: &MemberRecord) -> Result<Uuid>;
    async fn link_speakers_to_members(&self, parliament: &str) -> Result<u64>;
    async fn link_bill_sponsors_to_members(&self) -> Result<u64>;

    /// Return all (id, url) pairs for stored members: used by the profile import pass.
    async fn list_member_urls(&self) -> Result<Vec<(Uuid, String)>>;

    /// Enrich an existing member row with profile-page data.
    async fn update_member_profile(
        &self,
        member_id: Uuid,
        enrichment: &MemberProfileData,
    ) -> Result<()>;

    /*  Enrichment */

    /// Return up to `limit` (bill_mention, speaker) pairs that have
    /// contributions_text but no summary yet.
    async fn pending_bill_summaries(&self, limit: u32) -> Result<Vec<PendingBillSummary>>;

    /// Persist an AI-generated summary for a bill mention speaker row.
    async fn store_bill_mention_summary(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;

    async fn pending_topic_summaries(&self, limit: u32) -> Result<Vec<PendingTopicSummary>>;

    async fn store_topic_summary(
        &self,
        topic_id: Uuid,
        speaker_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;

    async fn pending_topic_appearance_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingTopicAppearanceSummary>>;

    async fn store_topic_appearance_summary(
        &self,
        topic_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;

    /* Bill node / journey / sitting enrichment */

    async fn pending_bill_appearance_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingBillAppearanceSummary>>;
    async fn store_bill_appearance_summary(
        &self,
        bill_mention_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;

    async fn pending_bill_journey_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingBillJourneySummary>>;
    async fn store_bill_journey_summary(
        &self,
        bill_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;

    async fn pending_sitting_summaries(&self, limit: u32) -> Result<Vec<PendingSittingSummary>>;
    async fn store_sitting_generated_summary(
        &self,
        sitting_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use odnelazm::DataSource;

    use super::{
        IngestionRunCompletion, IngestionRunStatus, MemberSourceIdentity, SittingSourceIdentity,
        normalize_member_name,
    };

    fn completion(status: IngestionRunStatus) -> IngestionRunCompletion {
        IngestionRunCompletion {
            status,
            discovered: 3,
            processed: 3,
            succeeded: 1,
            skipped: 1,
            failed: 1,
            error_message: Some("one failed".to_owned()),
            error_metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn ingestion_run_counts_require_one_outcome_per_processed_item() {
        assert!(completion(IngestionRunStatus::Partial).validate().is_ok());
        let mut invalid = completion(IngestionRunStatus::Partial);
        invalid.processed = 2;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn succeeded_ingestion_run_rejects_failure_metadata() {
        assert!(
            completion(IngestionRunStatus::Succeeded)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn current_old_slug_and_document_url_share_external_key() {
        let old = SittingSourceIdentity::from_observation(
            DataSource::Current,
            "https://mzalendo.com/democracy-tools/hansard/thursday-12th-february-2026-afternoon-sitting-2438/",
        );
        let redesigned = SittingSourceIdentity::from_observation(
            DataSource::Current,
            "/democracy-tools/hansard/document/2438/",
        );

        assert_eq!(old.external_key.as_deref(), Some("2438"));
        assert_eq!(old.external_key, redesigned.external_key);
        assert_eq!(redesigned.source_key, "mzalendo-current");
    }

    #[test]
    fn source_url_normalization_qualifies_paths_and_removes_non_identity_parts() {
        let identity = SittingSourceIdentity::from_observation(
            DataSource::Current,
            "/democracy-tools/hansard/document/3096/?download=1#transcript",
        );

        assert_eq!(
            identity.source_url,
            "https://mzalendo.com/democracy-tools/hansard/document/3096/?download=1#transcript"
        );
        assert_eq!(
            identity.normalized_url,
            "https://mzalendo.com/democracy-tools/hansard/document/3096"
        );
        assert_eq!(identity.external_key.as_deref(), Some("3096"));
    }

    #[test]
    fn archive_timestamp_is_not_an_external_key() {
        let identity = SittingSourceIdentity::from_observation(
            DataSource::Archive,
            "https://info.mzalendo.com/hansard/sitting/national_assembly/2025-07-01-14-30-00/",
        );

        assert_eq!(identity.source_key, "mzalendo-archive");
        assert_eq!(identity.external_key, None);
        assert_eq!(
            identity.normalized_url,
            "https://info.mzalendo.com/hansard/sitting/national_assembly/2025-07-01-14-30-00"
        );
    }

    #[test]
    fn unrelated_current_numeric_suffix_is_not_an_external_key() {
        let identity = SittingSourceIdentity::from_observation(
            DataSource::Current,
            "https://mzalendo.com/person/member-2438/",
        );

        assert_eq!(identity.external_key, None);
    }

    #[test]
    fn malformed_document_identifier_is_not_an_external_key() {
        for url in [
            "/democracy-tools/hansard/document/2438-extra/",
            "/democracy-tools/hansard/document/",
            "/democracy-tools/hansard/not-a-sitting/",
        ] {
            let identity = SittingSourceIdentity::from_observation(DataSource::Current, url);
            assert_eq!(identity.external_key, None, "unexpected key for {url}");
        }
    }

    #[test]
    fn current_member_relative_and_absolute_urls_share_identity() {
        let relative = MemberSourceIdentity::from_observation(
            DataSource::Current,
            "/mps-performance/national-assembly/13th-parliament/example-member/",
        );
        let absolute = MemberSourceIdentity::from_observation(
            DataSource::Current,
            "http://www.mzalendo.com/mps-performance/national-assembly/13th-parliament/example-member?tab=activity#top",
        );

        assert_eq!(relative.normalized_url, absolute.normalized_url);
        assert_eq!(relative.external_key.as_deref(), Some("example-member"));
        assert_eq!(relative.external_key, absolute.external_key);
        assert_eq!(relative.source_key, "mzalendo-current");
    }

    #[test]
    fn current_member_profile_routes_share_external_key() {
        let legacy =
            MemberSourceIdentity::from_observation(DataSource::Current, "/person/example-member/");
        let current = MemberSourceIdentity::from_observation(
            DataSource::Current,
            "/mps-performance/national-assembly/13th-parliament/example-member/",
        );

        assert_ne!(legacy.normalized_url, current.normalized_url);
        assert_eq!(legacy.external_key, current.external_key);
    }

    #[test]
    fn archive_member_relative_and_absolute_urls_share_identity() {
        let relative =
            MemberSourceIdentity::from_observation(DataSource::Archive, "/person/example-member/");
        let absolute = MemberSourceIdentity::from_observation(
            DataSource::Archive,
            "https://info.mzalendo.com/person/example-member/?ref=list",
        );

        assert_eq!(relative.normalized_url, absolute.normalized_url);
        assert_eq!(relative.external_key.as_deref(), Some("example-member"));
        assert_eq!(relative.source_key, "mzalendo-archive");
    }

    #[test]
    fn member_name_normalization_is_case_and_whitespace_insensitive() {
        assert_eq!(normalize_member_name("  Jane\n  W.  DOE "), "jane w. doe");
    }

    #[test]
    fn unrelated_member_url_has_no_external_key() {
        let identity = MemberSourceIdentity::from_observation(
            DataSource::Current,
            "/organizations/example-member/",
        );

        assert_eq!(identity.external_key, None);
    }
}
