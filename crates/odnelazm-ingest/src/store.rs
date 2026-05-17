use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use odnelazm::HansardSitting;

use crate::Result;

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
    pub house: String,
    pub parliament: String,
    pub role: Option<String>,
    pub constituency: Option<String>,
}

/// Enrichment data fetched from a member's individual profile page.
#[derive(Debug, Clone)]
pub struct MemberEnrichment {
    pub photo_url: Option<String>,
    pub biography: Option<String>,
    pub party: Option<String>,
    pub positions: Vec<String>,
    pub committees: Vec<String>,
    pub speeches_last_year: Option<u32>,
    pub speeches_total: Option<u32>,
    pub bills_total: Option<u32>,
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

    async fn upsert_sitting(&self, sitting: &HansardSitting) -> Result<Uuid>;
    async fn list_ingested_urls(&self) -> Result<Vec<String>>;
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
    async fn link_speakers_to_members(&self) -> Result<u64>;
    async fn link_bill_sponsors_to_members(&self) -> Result<u64>;

    /// Return all (id, url) pairs for stored members: used by the enrichment pass.
    async fn list_member_urls(&self) -> Result<Vec<(Uuid, String)>>;

    /// Enrich an existing member row with profile-page data.
    async fn enrich_member(&self, member_id: Uuid, enrichment: &MemberEnrichment) -> Result<()>;

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
