use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use odnelazm::HansardSitting;

use crate::Result;

/// A speaker extracted from a sitting's contributions.
#[derive(Debug, Clone)]
pub struct SpeakerRecord {
    /// Display name as it appears in the transcript.
    pub name: String,
    /// Relative mzalendo profile URL, when present in the contribution.
    pub url: Option<String>,
}

/// A bill identified by the extractor.
#[derive(Debug, Clone)]
pub struct BillRecord {
    /// Canonical, title-cased bill name used as the identity key.
    /// e.g. "Income Tax (Amendment) Bill"
    pub name: String,
    /// Formal bill number when extractable from contribution text.
    /// e.g. "National Assembly Bill No.20 of 2026"
    pub bill_number: Option<String>,
    pub year: Option<i32>,
    /// Name of the member who moved the bill (first mover in the debate).
    pub sponsor: Option<String>,
}

/// One appearance of a bill in one sitting at a particular legislative stage.
#[derive(Debug, Clone)]
pub struct BillMentionRecord {
    pub sitting_id: Uuid,
    pub house: String,
    pub date: NaiveDate,
    /// Legislative stage label, e.g. "Second Reading", "Committee Stage".
    pub stage: Option<String>,
    /// The section or subsection title this mention was found under.
    pub section_title: String,
    /// Number of contributions in this bill's debate segment.
    pub speech_count: u32,
}

/// Generic async datastore interface.
///
/// Implement this trait to plug in any backend (PostgreSQL, SQLite, in-memory,
/// etc.). Every method is idempotent — calling it multiple times with the same
/// data must not create duplicates.
#[async_trait]
pub trait DataStore: Send + Sync {
    /// Apply schema migrations. Safe to call on every startup.
    async fn migrate(&self) -> Result<()>;

    /// Persist a sitting (upsert on URL). Returns the sitting's UUID.
    async fn upsert_sitting(&self, sitting: &HansardSitting) -> Result<Uuid>;

    /// Return the URLs of all sittings already in the store (used to skip
    /// re-ingestion on subsequent pipeline runs).
    async fn list_ingested_urls(&self) -> Result<Vec<String>>;

    /// Attach a pre-computed embedding vector to a sitting.
    async fn store_sitting_embedding(&self, sitting_id: Uuid, embedding: Vec<f32>) -> Result<()>;

    /// Upsert a speaker (on name + url). Returns the speaker's UUID.
    async fn upsert_speaker(&self, speaker: &SpeakerRecord) -> Result<Uuid>;

    /// Record that a speaker was active in a sitting, with a speech count.
    async fn link_speaker_to_sitting(
        &self,
        speaker_id: Uuid,
        sitting_id: Uuid,
        speech_count: u32,
    ) -> Result<()>;

    /// Upsert a bill (on name). Returns the bill's UUID.
    async fn upsert_bill(&self, bill: &BillRecord) -> Result<Uuid>;

    /// Record one appearance of a bill in a sitting. Returns the bill_mention UUID.
    async fn upsert_bill_mention(&self, bill_id: Uuid, mention: &BillMentionRecord)
    -> Result<Uuid>;

    /// Record that a speaker contributed to a specific bill mention.
    async fn link_speaker_to_bill_mention(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
    ) -> Result<()>;
}
