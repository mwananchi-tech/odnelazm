use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use odnelazm::HansardSitting;

use crate::{
    Result,
    store::{BillMentionRecord, BillRecord, DataStore, MemberRecord, SpeakerRecord},
};

const MIGRATIONS: &str = concat!(
    include_str!("../migrations/0001_init.sql"),
    "\n",
    include_str!("../migrations/0002_bill_speakers.sql"),
    "\n",
    include_str!("../migrations/0004_members.sql"),
);

#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
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

        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO sittings
                (id, url, house, date, session_type, source, summary, sentiment, pdf_url, raw_json)
            VALUES
                (uuid_generate_v4(), $1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (url) DO UPDATE SET
                summary      = COALESCE(EXCLUDED.summary,   sittings.summary),
                sentiment    = COALESCE(EXCLUDED.sentiment, sittings.sentiment),
                pdf_url      = COALESCE(EXCLUDED.pdf_url,   sittings.pdf_url),
                raw_json     = EXCLUDED.raw_json
            RETURNING id
            "#,
        )
        .bind(&sitting.url)
        .bind(&house)
        .bind(sitting.date)
        .bind(&sitting.session_type)
        .bind(&source)
        .bind(sitting.summary.as_deref())
        .bind(sitting.sentiment.as_deref())
        .bind(sitting.pdf_url.as_deref())
        .bind(&raw_json)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    async fn list_ingested_urls(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT url FROM sittings")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn store_sitting_embedding(&self, sitting_id: Uuid, embedding: Vec<f32>) -> Result<()> {
        // Store as JSON array; swap to pgvector REAL[] once the extension is added.
        let json = serde_json::to_value(&embedding)?;
        sqlx::query("UPDATE sittings SET embedding = $1::jsonb WHERE id = $2")
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

    async fn upsert_member(&self, member: &MemberRecord) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO members (id, name, url, house, parliament, role, constituency)
            VALUES (uuid_generate_v4(), $1, $2, $3, $4, $5, $6)
            ON CONFLICT (url) DO UPDATE SET
                name         = EXCLUDED.name,
                role         = COALESCE(EXCLUDED.role, members.role),
                constituency = COALESCE(EXCLUDED.constituency, members.constituency)
            RETURNING id
            "#,
        )
        .bind(&member.name)
        .bind(&member.url)
        .bind(&member.house)
        .bind(&member.parliament)
        .bind(member.role.as_deref())
        .bind(member.constituency.as_deref())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn link_speakers_to_members(&self) -> Result<u64> {
        let url_linked: (i64,) = sqlx::query_as("SELECT link_speakers_by_url()")
            .fetch_one(&self.pool)
            .await?;

        let name_linked: (i64,) = sqlx::query_as("SELECT link_speakers_by_name(0.45)")
            .fetch_one(&self.pool)
            .await?;

        Ok((url_linked.0 + name_linked.0) as u64)
    }

    async fn link_speaker_to_bill_mention(
        &self,
        bill_mention_id: Uuid,
        speaker_id: Uuid,
        speech_count: u32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO bill_mention_speakers (bill_mention_id, speaker_id, speech_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (bill_mention_id, speaker_id) DO UPDATE
                SET speech_count = bill_mention_speakers.speech_count + EXCLUDED.speech_count
            "#,
        )
        .bind(bill_mention_id)
        .bind(speaker_id)
        .bind(speech_count as i32)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
