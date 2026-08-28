use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use chrono::NaiveDate;
use odnelazm::HansardSitting;

use crate::{
    Result,
    store::{
        BillMentionContext, BillMentionRecord, BillRecord, DataStore, MemberProfileData,
        MemberRecord, MemberSourceIdentity, PendingBillAppearanceSummary,
        PendingBillJourneySummary, PendingBillSummary, PendingSittingSummary,
        PendingTopicAppearanceSummary, PendingTopicSummary, SittingSourceIdentity, SpeakerRecord,
        TopicRecord, normalize_member_name,
    },
};

#[cfg(test)]
const SOURCE_IDENTITY_MIGRATION: &str =
    include_str!("../migrations/0015_source_identity_foundation.sql");

const MIGRATIONS: &str = concat!(
    include_str!("../migrations/0001_init.sql"),
    "\n",
    include_str!("../migrations/0002_bill_speakers.sql"),
    "\n",
    include_str!("../migrations/0004_members.sql"),
    "\n",
    include_str!("../migrations/0005_topics.sql"),
    "\n",
    include_str!("../migrations/0006_contribution_text.sql"),
    "\n",
    include_str!("../migrations/0007_member_enrichment.sql"),
    "\n",
    include_str!("../migrations/0008_fix_bill_mentions_unique.sql"),
    "\n",
    include_str!("../migrations/0009_bill_sponsor_id.sql"),
    "\n",
    include_str!("../migrations/0010_bill_summary.sql"),
    "\n",
    include_str!("../migrations/0011_sitting_youtube.sql"),
    "\n",
    include_str!("../migrations/0012_sitting_generated_summary.sql"),
    "\n",
    include_str!("../migrations/0013_summary_model.sql"),
    "\n",
    include_str!("../migrations/0014_topic_summary.sql"),
    "\n",
    include_str!("../migrations/0015_source_identity_foundation.sql"),
);

const UPDATE_SITTING_SQL: &str = r#"
    UPDATE sittings SET
        summary   = COALESCE($1, summary),
        sentiment = COALESCE($2, sentiment),
        pdf_url   = COALESCE($3, pdf_url),
        raw_json  = $4
    WHERE id = $5
"#;

const INSERT_SITTING_SQL: &str = r#"
    INSERT INTO sittings
        (id, url, house, date, session_type, source, summary, sentiment, pdf_url, raw_json)
    VALUES
        (uuid_generate_v4(), $1, $2, $3, $4, $5, $6, $7, $8, $9)
    ON CONFLICT (url) DO UPDATE SET
        summary   = COALESCE(EXCLUDED.summary, sittings.summary),
        sentiment = COALESCE(EXCLUDED.sentiment, sittings.sentiment),
        pdf_url   = COALESCE(EXCLUDED.pdf_url, sittings.pdf_url),
        raw_json  = EXCLUDED.raw_json
    RETURNING id
"#;

const FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL: &str = r#"
    SELECT member_id, id
    FROM member_sources
    WHERE data_source_id = $1 AND external_key = $2
"#;

const FIND_MEMBER_SOURCE_BY_URL_SQL: &str = r#"
    SELECT member_id, id
    FROM member_sources
    WHERE data_source_id = $1 AND normalized_url = $2
"#;

const FIND_CANONICAL_MEMBER_SQL: &str = r#"
    SELECT id
    FROM members
    WHERE lower(regexp_replace(btrim(name), '\s+', ' ', 'g')) = $1
      AND house = $2
      AND parliament = $3
      AND constituency IS NOT DISTINCT FROM $4
    LIMIT 2
    FOR UPDATE
"#;

const UPDATE_CANONICAL_MEMBER_SQL: &str = r#"
    UPDATE members SET
        name         = $1,
        role         = COALESCE($2, role),
        constituency = COALESCE($3, constituency)
    WHERE id = $4
"#;

const INSERT_MEMBER_SOURCE_SQL: &str = r#"
    INSERT INTO member_sources
        (member_id, data_source_id, external_key, source_url, normalized_url, raw_metadata)
    VALUES ($1, $2, $3, $4, $5, $6)
"#;

#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        // persistent(false) on every query uses PostgreSQL's unnamed prepared statement slot,
        // which the server overwrites on each use rather than accumulating named statements
        // (sqlx_s_1, sqlx_s_2, ...) that persist across connection reuse by a pooler.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DataStore for PostgresStore {
    async fn migrate(&self) -> Result<()> {
        sqlx::raw_sql(MIGRATIONS).execute(&self.pool).await?;
        Ok(())
    }

    async fn upsert_sitting(&self, sitting: &HansardSitting) -> Result<Uuid> {
        let raw_json = serde_json::to_value(sitting)?;
        let house = sitting.house.to_string();
        let source = sitting.source.to_string();
        let identity = SittingSourceIdentity::from_sitting(sitting);
        let mut tx = self.pool.begin().await?;

        // Serializing per source closes the resolve-then-insert race without
        // imposing a canonical identity constraint on the legacy sittings table.
        let (data_source_id,): (Uuid,) =
            sqlx::query_as("SELECT id FROM data_sources WHERE source_key = $1 FOR UPDATE")
                .persistent(false)
                .bind(&identity.source_key)
                .fetch_one(&mut *tx)
                .await?;

        let mut resolved: Option<(Uuid, Uuid)> = None;
        if let Some(external_key) = identity.external_key.as_deref() {
            resolved = sqlx::query_as(
                "SELECT sitting_id, id FROM sitting_sources WHERE data_source_id = $1 AND external_key = $2",
            )
            .persistent(false)
            .bind(data_source_id)
            .bind(external_key)
            .fetch_optional(&mut *tx)
            .await?;
        }
        if resolved.is_none() {
            resolved = sqlx::query_as(
                "SELECT sitting_id, id FROM sitting_sources WHERE data_source_id = $1 AND normalized_url = $2",
            )
            .persistent(false)
            .bind(data_source_id)
            .bind(&identity.normalized_url)
            .fetch_optional(&mut *tx)
            .await?;
        }

        let (sitting_id, sitting_source_id) = if let Some((sitting_id, source_id)) = resolved {
            sqlx::query(UPDATE_SITTING_SQL)
                .persistent(false)
                .bind(sitting.summary.as_deref())
                .bind(sitting.sentiment.as_deref())
                .bind(sitting.pdf_url.as_deref())
                .bind(&raw_json)
                .bind(sitting_id)
                .execute(&mut *tx)
                .await?;
            (sitting_id, Some(source_id))
        } else {
            let (sitting_id,): (Uuid,) = sqlx::query_as(INSERT_SITTING_SQL)
                .persistent(false)
                .bind(&identity.source_url)
                .bind(&house)
                .bind(sitting.date)
                .bind(&sitting.session_type)
                .bind(&source)
                .bind(sitting.summary.as_deref())
                .bind(sitting.sentiment.as_deref())
                .bind(sitting.pdf_url.as_deref())
                .bind(&raw_json)
                .fetch_one(&mut *tx)
                .await?;
            (sitting_id, None)
        };

        if let Some(source_id) = sitting_source_id {
            sqlx::query(
                r#"
                UPDATE sitting_sources SET
                    source_url = $1, normalized_url = $2,
                    external_key = COALESCE($3, external_key), last_seen_at = now()
                WHERE id = $4
                "#,
            )
            .persistent(false)
            .bind(&identity.source_url)
            .bind(&identity.normalized_url)
            .bind(identity.external_key.as_deref())
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO sitting_sources
                    (sitting_id, data_source_id, external_key, source_url, normalized_url)
                VALUES ($1, $2, $3, $4, $5)
                "#,
            )
            .persistent(false)
            .bind(sitting_id)
            .bind(data_source_id)
            .bind(identity.external_key.as_deref())
            .bind(&identity.source_url)
            .bind(&identity.normalized_url)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(sitting_id)
    }

    async fn list_ingested_sitting_sources(&self) -> Result<Vec<SittingSourceIdentity>> {
        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT ds.source_key, ss.source_url,
                   COALESCE(ss.normalized_url, regexp_replace(ss.source_url, '/+$', '')),
                   ss.external_key
            FROM sitting_sources ss
            JOIN data_sources ds ON ds.id = ss.data_source_id
            "#,
        )
        .persistent(false)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(source_key, source_url, normalized_url, external_key)| SittingSourceIdentity {
                    source_key,
                    source_url,
                    normalized_url,
                    external_key,
                },
            )
            .collect())
    }

    async fn store_sitting_embedding(&self, sitting_id: Uuid, embedding: Vec<f32>) -> Result<()> {
        // Store as JSON array; swap to pgvector REAL[] once the extension is added.
        let json = serde_json::to_value(&embedding)?;
        sqlx::query("UPDATE sittings SET embedding = $1::jsonb WHERE id = $2")
            .persistent(false)
            .bind(&json)
            .bind(sitting_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn upsert_speaker(&self, speaker: &SpeakerRecord) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO speakers (id, name, url)
            VALUES (uuid_generate_v4(), $1, $2)
            ON CONFLICT (name, url) DO UPDATE SET name = EXCLUDED.name
            RETURNING id
            "#,
        )
        .persistent(false)
        .bind(&speaker.name)
        .bind(speaker.url.as_deref())
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    async fn link_speaker_to_sitting(
        &self,
        speaker_id: Uuid,
        sitting_id: Uuid,
        speech_count: u32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO sitting_speakers (sitting_id, speaker_id, speech_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (sitting_id, speaker_id) DO UPDATE
                SET speech_count = sitting_speakers.speech_count + EXCLUDED.speech_count
            "#,
        )
        .persistent(false)
        .bind(sitting_id)
        .bind(speaker_id)
        .bind(speech_count as i32)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn upsert_bill(&self, bill: &BillRecord) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO bills (id, name, bill_number, year, sponsor, updated_at)
            VALUES (uuid_generate_v4(), $1, $2, $3, $4, now())
            ON CONFLICT (name) DO UPDATE SET
                bill_number = COALESCE(EXCLUDED.bill_number, bills.bill_number),
                year        = COALESCE(EXCLUDED.year,        bills.year),
                sponsor     = COALESCE(EXCLUDED.sponsor,     bills.sponsor),
                updated_at  = now()
            RETURNING id
            "#,
        )
        .persistent(false)
        .bind(&bill.name)
        .bind(bill.bill_number.as_deref())
        .bind(bill.year)
        .bind(bill.sponsor.as_deref())
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    async fn upsert_bill_mention(
        &self,
        bill_id: Uuid,
        mention: &BillMentionRecord,
    ) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO bill_mentions
                (id, bill_id, sitting_id, house, date, stage, section_title, speech_count)
            VALUES
                (uuid_generate_v4(), $1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (bill_id, sitting_id, stage) DO UPDATE SET
                speech_count  = EXCLUDED.speech_count,
                section_title = EXCLUDED.section_title
            RETURNING id
            "#,
        )
        .persistent(false)
        .bind(bill_id)
        .bind(mention.sitting_id)
        .bind(&mention.house)
        .bind(mention.date)
        .bind(mention.stage.as_deref())
        .bind(&mention.section_title)
        .bind(mention.speech_count as i32)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn upsert_topic(&self, topic: &TopicRecord) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO topics (id, sitting_id, section_type, title, speech_count)
            VALUES (uuid_generate_v4(), $1, $2, $3, $4)
            ON CONFLICT (sitting_id, section_type, title) DO UPDATE SET
                speech_count = EXCLUDED.speech_count
            RETURNING id
            "#,
        )
        .persistent(false)
        .bind(topic.sitting_id)
        .bind(&topic.section_type)
        .bind(&topic.title)
        .bind(topic.speech_count as i32)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn link_speaker_to_topic(
        &self,
        topic_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
        contributions_text: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO topic_speakers (topic_id, speaker_id, speech_count, contributions_text)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (topic_id, speaker_id) DO UPDATE SET
                speech_count       = EXCLUDED.speech_count,
                contributions_text = EXCLUDED.contributions_text
            "#,
        )
        .persistent(false)
        .bind(topic_id)
        .bind(speaker_id)
        .bind(speech_count as i32)
        .bind(contributions_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn pending_bill_summaries(&self, limit: u32) -> Result<Vec<PendingBillSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                Option<String>,
                String,
                NaiveDate,
                String,
                Option<String>,
                String,
            ),
        >(
            r#"
            SELECT bms.bill_mention_id, bms.speaker_id,
                   m.name, b.name, bm.date, bm.house, bm.stage,
                   bms.contributions_text
            FROM bill_mention_speakers bms
            JOIN bill_mentions bm ON bm.id = bms.bill_mention_id
            JOIN bills b          ON b.id  = bm.bill_id
            JOIN speakers sp      ON sp.id = bms.speaker_id
            LEFT JOIN members m   ON m.id  = sp.member_id
            WHERE bms.contributions_text IS NOT NULL
              AND bms.summary IS NULL
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    bill_mention_id,
                    speaker_id,
                    member_name,
                    bill_name,
                    date,
                    house,
                    stage,
                    contributions_text,
                )| {
                    PendingBillSummary {
                        bill_mention_id,
                        speaker_id,
                        member_name,
                        bill_name,
                        date,
                        house,
                        stage,
                        contributions_text,
                    }
                },
            )
            .collect())
    }

    async fn store_bill_mention_summary(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE bill_mention_speakers SET summary = $1, summary_model = $2 WHERE bill_mention_id = $3 AND speaker_id = $4",
        )
        .persistent(false)
        .bind(summary)
        .bind(model)
        .bind(bill_mention_id)
        .bind(speaker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn pending_topic_summaries(&self, limit: u32) -> Result<Vec<PendingTopicSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                Option<String>,
                String,
                String,
                NaiveDate,
                String,
                String,
            ),
        >(
            r#"
            SELECT ts.topic_id, ts.speaker_id,
                   m.name, t.title, t.section_type, s.date, s.house,
                   ts.contributions_text
            FROM topic_speakers ts
            JOIN topics t         ON t.id  = ts.topic_id
            JOIN sittings s       ON s.id  = t.sitting_id
            JOIN speakers sp      ON sp.id = ts.speaker_id
            LEFT JOIN members m   ON m.id  = sp.member_id
            WHERE ts.contributions_text IS NOT NULL
              AND ts.summary IS NULL
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    topic_id,
                    speaker_id,
                    member_name,
                    topic_title,
                    section_type,
                    date,
                    house,
                    contributions_text,
                )| {
                    PendingTopicSummary {
                        topic_id,
                        speaker_id,
                        member_name,
                        topic_title,
                        section_type,
                        date,
                        house,
                        contributions_text,
                    }
                },
            )
            .collect())
    }

    async fn store_topic_summary(
        &self,
        topic_id: Uuid,
        speaker_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE topic_speakers SET summary = $1, summary_model = $2 WHERE topic_id = $3 AND speaker_id = $4",
        )
        .persistent(false)
        .bind(summary)
        .bind(model)
        .bind(topic_id)
        .bind(speaker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn pending_topic_appearance_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingTopicAppearanceSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                NaiveDate,
                String,
                String,
                serde_json::Value,
            ),
        >(
            r#"
            SELECT t.id, t.title, t.section_type, s.date, s.house, s.session_type, s.raw_json
            FROM topics t
            JOIN sittings s ON s.id = t.sitting_id
            WHERE t.summary IS NULL AND t.speech_count > 0
            ORDER BY s.date DESC
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, title, section_type, date, house, session_type, raw_json)| {
                    PendingTopicAppearanceSummary {
                        topic_id: id,
                        title,
                        section_type,
                        date,
                        house,
                        session_type,
                        sitting_raw_json: raw_json,
                    }
                },
            )
            .collect())
    }

    async fn store_topic_appearance_summary(
        &self,
        topic_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE topics SET summary = $1, summary_model = $2 WHERE id = $3")
            .persistent(false)
            .bind(summary)
            .bind(model)
            .bind(topic_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn upsert_member(&self, member: &MemberRecord) -> Result<Uuid> {
        let identity = MemberSourceIdentity::from_member(member);
        let mut tx = self.pool.begin().await?;
        let (data_source_id,): (Uuid,) =
            sqlx::query_as("SELECT id FROM data_sources WHERE source_key = $1 FOR UPDATE")
                .persistent(false)
                .bind(&identity.source_key)
                .fetch_one(&mut *tx)
                .await?;

        let mut resolved: Option<(Uuid, Uuid)> = None;
        if let Some(external_key) = identity.external_key.as_deref() {
            resolved = sqlx::query_as(FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL)
                .persistent(false)
                .bind(data_source_id)
                .bind(external_key)
                .fetch_optional(&mut *tx)
                .await?;
        }
        if resolved.is_none() {
            resolved = sqlx::query_as(FIND_MEMBER_SOURCE_BY_URL_SQL)
                .persistent(false)
                .bind(data_source_id)
                .bind(&identity.normalized_url)
                .fetch_optional(&mut *tx)
                .await?;
        }

        let (member_id, member_source_id) = if let Some((member_id, source_id)) = resolved {
            (member_id, Some(source_id))
        } else {
            let candidates: Vec<(Uuid,)> = sqlx::query_as(FIND_CANONICAL_MEMBER_SQL)
                .persistent(false)
                .bind(normalize_member_name(&member.name))
                .bind(&member.house)
                .bind(&member.parliament)
                .bind(member.constituency.as_deref())
                .fetch_all(&mut *tx)
                .await?;

            if let [(member_id,)] = candidates.as_slice() {
                (*member_id, None)
            } else {
                let (member_id,): (Uuid,) = sqlx::query_as(
                    r#"
                    INSERT INTO members (id, name, url, house, parliament, role, constituency)
                    VALUES (uuid_generate_v4(), $1, $2, $3, $4, $5, $6)
                    RETURNING id
                    "#,
                )
                .persistent(false)
                .bind(&member.name)
                .bind(&member.url)
                .bind(&member.house)
                .bind(&member.parliament)
                .bind(member.role.as_deref())
                .bind(member.constituency.as_deref())
                .fetch_one(&mut *tx)
                .await?;
                (member_id, None)
            }
        };

        sqlx::query(UPDATE_CANONICAL_MEMBER_SQL)
            .persistent(false)
            .bind(&member.name)
            .bind(member.role.as_deref())
            .bind(member.constituency.as_deref())
            .bind(member_id)
            .execute(&mut *tx)
            .await?;

        let raw_metadata = serde_json::json!({
            "house": member.house,
            "parliament": member.parliament,
            "role": member.role,
            "constituency": member.constituency,
        });
        if let Some(source_id) = member_source_id {
            sqlx::query(
                r#"
                UPDATE member_sources SET
                    source_url = $1,
                    normalized_url = $2,
                    external_key = COALESCE($3, external_key),
                    raw_metadata = $4,
                    last_seen_at = now()
                WHERE id = $5
                "#,
            )
            .persistent(false)
            .bind(&identity.source_url)
            .bind(&identity.normalized_url)
            .bind(identity.external_key.as_deref())
            .bind(&raw_metadata)
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(INSERT_MEMBER_SOURCE_SQL)
                .persistent(false)
                .bind(member_id)
                .bind(data_source_id)
                .bind(identity.external_key.as_deref())
                .bind(&identity.source_url)
                .bind(&identity.normalized_url)
                .bind(&raw_metadata)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(member_id)
    }

    async fn list_member_urls(&self) -> Result<Vec<(Uuid, String)>> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, url FROM members ORDER BY name")
            .persistent(false)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    async fn update_member_profile(&self, member_id: Uuid, e: &MemberProfileData) -> Result<()> {
        let positions = serde_json::to_value(&e.positions)?;
        let committees = serde_json::to_value(&e.committees)?;
        sqlx::query(
            r#"
            UPDATE members SET
                photo_url          = COALESCE($1, photo_url),
                biography          = COALESCE($2, biography),
                party              = COALESCE($3, party),
                positions          = COALESCE($4, positions),
                committees         = COALESCE($5, committees),
                speeches_last_year = COALESCE($6, speeches_last_year),
                speeches_total     = COALESCE($7, speeches_total),
                bills_total        = COALESCE($8, bills_total)
            WHERE id = $9
            "#,
        )
        .persistent(false)
        .bind(e.photo_url.as_deref())
        .bind(e.biography.as_deref())
        .bind(e.party.as_deref())
        .bind(&positions)
        .bind(&committees)
        .bind(e.speeches_last_year.map(|v| v as i32))
        .bind(e.speeches_total.map(|v| v as i32))
        .bind(e.bills_total.map(|v| v as i32))
        .bind(member_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn link_speakers_to_members(&self, parliament: &str) -> Result<u64> {
        // Both functions are defined in migrations/0004_members.sql.
        // link_speakers_by_url: exact URL match between speakers.url and members.url.
        // link_speakers_by_name: fuzzy trigram match via clean_speaker_name() and match_member().
        let url_linked: (i64,) = sqlx::query_as("SELECT link_speakers_by_url()")
            .persistent(false)
            .fetch_one(&self.pool)
            .await?;
        let name_linked: (i64,) = sqlx::query_as("SELECT link_speakers_by_name(0.45)")
            .persistent(false)
            .fetch_one(&self.pool)
            .await?;

        // Link NA presiding officers by role. "Hon. Speaker" / "Hon Speaker" are recorded
        // without a URL and won't fuzzy-match because "Speaker" is too generic. We resolve
        // them via the role column instead, scoped to NA sittings and the given parliament.
        // TODO: scope by sitting date range rather than parliament string when adding
        // historical data, to handle Speaker changes across parliaments correctly.
        let role_linked = sqlx::query_scalar::<_, i64>(
            r#"
            WITH updated AS (
                UPDATE speakers sp
                SET    member_id = m.id
                FROM   members m
                WHERE  m.parliament = $1
                  AND  m.house      = 'National Assembly'
                  AND  sp.member_id IS NULL
                  AND  (
                         (m.role = 'Speaker'
                          AND sp.name ~* '^hon\.?\s+speaker$')
                         OR
                         (m.role ILIKE '%deputy speaker%'
                          AND sp.name ~* '^(the|hon\.?)\s+deputy\s+speaker$')
                       )
                  AND  EXISTS (
                         SELECT 1
                         FROM   sitting_speakers ss
                         JOIN   sittings s ON s.id = ss.sitting_id
                         WHERE  ss.speaker_id = sp.id
                           AND  s.house = 'National Assembly'
                       )
                RETURNING 1
            )
            SELECT count(*)::BIGINT FROM updated
            "#,
        )
        .persistent(false)
        .bind(parliament)
        .fetch_one(&self.pool)
        .await?;

        Ok((url_linked.0 + name_linked.0 + role_linked) as u64)
    }

    async fn link_bill_sponsors_to_members(&self) -> Result<u64> {
        let linked = sqlx::query_scalar::<_, i64>(
            r#"
            WITH updated AS (
                UPDATE bills b
                SET sponsor_id = (
                    SELECT id FROM match_member(b.sponsor, 0.4)
                    ORDER BY score DESC
                    LIMIT 1
                )
                WHERE b.sponsor IS NOT NULL
                  AND b.sponsor_id IS NULL
                RETURNING 1
            )
            SELECT count(*)::BIGINT FROM updated
            "#,
        )
        .persistent(false)
        .fetch_one(&self.pool)
        .await?;
        Ok(linked as u64)
    }

    async fn link_speaker_to_bill_mention(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
        contributions_text: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO bill_mention_speakers (bill_mention_id, speaker_id, speech_count, contributions_text)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (bill_mention_id, speaker_id) DO UPDATE SET
                speech_count       = EXCLUDED.speech_count,
                contributions_text = EXCLUDED.contributions_text
            "#,
        )
        .persistent(false)
        .bind(bill_mention_id)
        .bind(speaker_id)
        .bind(speech_count as i32)
        .bind(contributions_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn pending_bill_appearance_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingBillAppearanceSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                Option<String>,
                Option<String>,
                String,
                NaiveDate,
                String,
                String,
                serde_json::Value,
            ),
        >(
            r#"
            SELECT bm.id, b.name, b.bill_number, bm.stage, bm.section_title,
                   bm.date, bm.house, s.session_type, s.raw_json
            FROM bill_mentions bm
            JOIN bills b ON b.id = bm.bill_id
            JOIN sittings s ON s.id = bm.sitting_id
            WHERE bm.summary IS NULL AND bm.speech_count > 0
            ORDER BY bm.date DESC
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    bill_name,
                    bill_number,
                    stage,
                    section_title,
                    date,
                    house,
                    session_type,
                    raw_json,
                )| {
                    PendingBillAppearanceSummary {
                        bill_mention_id: id,
                        bill_name,
                        bill_number,
                        stage,
                        section_title,
                        date,
                        house,
                        session_type,
                        sitting_raw_json: raw_json,
                    }
                },
            )
            .collect())
    }

    async fn store_bill_appearance_summary(
        &self,
        bill_mention_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE bill_mentions SET summary = $1, summary_model = $2 WHERE id = $3")
            .persistent(false)
            .bind(summary)
            .bind(model)
            .bind(bill_mention_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn pending_bill_journey_summaries(
        &self,
        limit: u32,
    ) -> Result<Vec<PendingBillJourneySummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                Option<String>,
                Option<i32>,
                Option<String>,
                serde_json::Value,
            ),
        >(
            r#"
            SELECT b.id, b.name, b.bill_number, b.year, b.sponsor,
                   json_agg(
                     json_build_object(
                       'date',          bm.date,
                       'house',         bm.house,
                       'stage',         bm.stage,
                       'section_title', bm.section_title,
                       'summary',       bm.summary,
                       'speakers_text', (
                         SELECT string_agg(bms.contributions_text, E'\n\n')
                         FROM bill_mention_speakers bms
                         WHERE bms.bill_mention_id = bm.id
                           AND bms.contributions_text IS NOT NULL
                       )
                     )
                     ORDER BY bm.date
                   ) AS mentions
            FROM bills b
            JOIN bill_mentions bm ON bm.bill_id = b.id
            WHERE b.summary IS NULL
            GROUP BY b.id
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|(id, name, number, year, sponsor, mentions_json)| {
                let mentions: Vec<BillMentionContext> =
                    serde_json::from_value(mentions_json).ok()?;
                Some(PendingBillJourneySummary {
                    bill_id: id,
                    bill_name: name,
                    bill_number: number,
                    year,
                    sponsor,
                    mentions,
                })
            })
            .collect())
    }

    async fn store_bill_journey_summary(
        &self,
        bill_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE bills SET summary = $1, summary_model = $2 WHERE id = $3")
            .persistent(false)
            .bind(summary)
            .bind(model)
            .bind(bill_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn pending_sitting_summaries(&self, limit: u32) -> Result<Vec<PendingSittingSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                NaiveDate,
                String,
                String,
                Option<String>,
                serde_json::Value,
            ),
        >(
            r#"
            SELECT id, url, date, house, session_type, summary AS existing_summary, raw_json
            FROM sittings
            WHERE generated_summary IS NULL
            ORDER BY date DESC
            LIMIT $1
            "#,
        )
        .persistent(false)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, url, date, house, session_type, existing_summary, raw_json)| {
                    PendingSittingSummary {
                        sitting_id: id,
                        url,
                        date,
                        house,
                        session_type,
                        existing_summary,
                        raw_json,
                    }
                },
            )
            .collect())
    }

    async fn store_sitting_generated_summary(
        &self,
        sitting_id: Uuid,
        summary: &str,
        model: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE sittings SET generated_summary = $1, generated_summary_model = $2 WHERE id = $3")
            .persistent(false)
            .bind(summary).bind(model).bind(sitting_id).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FIND_CANONICAL_MEMBER_SQL, FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL,
        FIND_MEMBER_SOURCE_BY_URL_SQL, INSERT_MEMBER_SOURCE_SQL, INSERT_SITTING_SQL, MIGRATIONS,
        SOURCE_IDENTITY_MIGRATION, UPDATE_CANONICAL_MEMBER_SQL, UPDATE_SITTING_SQL,
    };

    #[test]
    fn source_identity_migration_is_in_runtime_migrations() {
        assert!(MIGRATIONS.ends_with(SOURCE_IDENTITY_MIGRATION));
    }

    #[test]
    fn source_identity_migration_covers_required_tables_and_summary_lifecycle() {
        for table in [
            "data_sources",
            "ingestion_runs",
            "sitting_sources",
            "member_sources",
        ] {
            assert!(
                SOURCE_IDENTITY_MIGRATION.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
                "missing idempotent creation for {table}"
            );
        }

        for table in [
            "sittings",
            "bills",
            "bill_mentions",
            "bill_mention_speakers",
            "topics",
            "topic_speakers",
        ] {
            let alter = format!("ALTER TABLE {table}");
            assert!(
                SOURCE_IDENTITY_MIGRATION.contains(&alter),
                "missing summary lifecycle metadata for {table}"
            );
        }

        assert!(SOURCE_IDENTITY_MIGRATION.contains("'mzalendo-current'"));
        assert!(SOURCE_IDENTITY_MIGRATION.contains("'mzalendo-archive'"));
        assert!(SOURCE_IDENTITY_MIGRATION.contains("INSERT INTO member_sources"));
        assert!(SOURCE_IDENTITY_MIGRATION.contains("external_key, source_url, normalized_url"));
        assert!(SOURCE_IDENTITY_MIGRATION.contains("ON CONFLICT DO NOTHING"));
    }

    #[test]
    fn canonical_sitting_updates_do_not_overwrite_derived_or_lifecycle_data() {
        for protected_column in [
            "generated_summary",
            "generated_summary_model",
            "embedding",
            "generated_summary_input_hash",
            "generated_summary_prompt_version",
            "generated_summary_generated_at",
            "generated_summary_stale_at",
            "ingested_at",
        ] {
            assert!(
                !UPDATE_SITTING_SQL.contains(protected_column),
                "canonical update touches {protected_column}"
            );
            assert!(
                !INSERT_SITTING_SQL.contains(protected_column),
                "legacy URL conflict update touches {protected_column}"
            );
        }

        assert!(UPDATE_SITTING_SQL.contains("WHERE id = $5"));
        assert!(INSERT_SITTING_SQL.contains("ON CONFLICT (url) DO UPDATE"));
    }

    #[test]
    fn member_resolution_sql_uses_source_aliases_and_a_unique_strict_fallback() {
        assert!(FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL.contains("member_sources"));
        assert!(FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL.contains("data_source_id = $1"));
        assert!(FIND_MEMBER_SOURCE_BY_EXTERNAL_KEY_SQL.contains("external_key = $2"));
        assert!(FIND_MEMBER_SOURCE_BY_URL_SQL.contains("normalized_url = $2"));

        for predicate in [
            "lower(regexp_replace(btrim(name), '\\s+', ' ', 'g')) = $1",
            "house = $2",
            "parliament = $3",
            "constituency IS NOT DISTINCT FROM $4",
            "LIMIT 2",
        ] {
            assert!(
                FIND_CANONICAL_MEMBER_SQL.contains(predicate),
                "missing conservative member predicate: {predicate}"
            );
        }
        assert!(!FIND_CANONICAL_MEMBER_SQL.contains("similarity"));
        assert!(INSERT_MEMBER_SOURCE_SQL.contains("member_sources"));
    }

    #[test]
    fn member_match_updates_preserve_uuid_url_and_profile_enrichment() {
        assert!(UPDATE_CANONICAL_MEMBER_SQL.contains("WHERE id = $4"));
        for protected_column in [
            "url",
            "photo_url",
            "biography",
            "party",
            "positions",
            "committees",
            "speeches_last_year",
            "speeches_total",
            "bills_total",
        ] {
            assert!(
                !UPDATE_CANONICAL_MEMBER_SQL.contains(protected_column),
                "member match update touches {protected_column}"
            );
        }
    }
}
