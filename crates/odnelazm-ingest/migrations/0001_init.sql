CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Raw sitting transcripts. raw_json holds the full HansardSitting payload so
-- nothing is lost; the other columns are indexed projections for fast querying.
CREATE TABLE IF NOT EXISTS sittings (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    url          TEXT        NOT NULL UNIQUE,
    house        TEXT        NOT NULL,
    date         DATE        NOT NULL,
    session_type TEXT        NOT NULL,
    source       TEXT        NOT NULL,
    summary      TEXT,
    sentiment    TEXT,
    pdf_url      TEXT,
    raw_json     JSONB       NOT NULL,
    -- Embeddings stored as a JSON float array. Migrate to pgvector REAL[] once
    -- the extension is available for efficient similarity search.
    embedding    JSONB,
    ingested_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sittings_date_idx   ON sittings (date);
CREATE INDEX IF NOT EXISTS sittings_house_idx  ON sittings (house);
CREATE INDEX IF NOT EXISTS sittings_source_idx ON sittings (source);

-- People who spoke in a sitting, extracted directly from contribution records.
-- url links back to their mzalendo profile when available.
CREATE TABLE IF NOT EXISTS speakers (
    id   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    url  TEXT,
    UNIQUE (name, url)
);

-- Join table: which speakers were active in which sittings, and how many times.
CREATE TABLE IF NOT EXISTS sitting_speakers (
    sitting_id   UUID NOT NULL REFERENCES sittings(id)  ON DELETE CASCADE,
    speaker_id   UUID NOT NULL REFERENCES speakers(id)  ON DELETE CASCADE,
    speech_count INT  NOT NULL DEFAULT 1,
    PRIMARY KEY (sitting_id, speaker_id)
);

-- One row per distinct bill. name is the canonical identity key because
-- bill_number is not always extractable from transcript text.
CREATE TABLE IF NOT EXISTS bills (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        TEXT        NOT NULL UNIQUE,
    bill_number TEXT,
    year        INT,
    sponsor     TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Each row is one appearance of a bill in one sitting at a particular stage.
-- A bill may appear multiple times in the same sitting (e.g. Second Reading
-- then Committee Stage on the same day), distinguished by stage.
--
-- The (bill_id, sitting_id, stage) triplet forms the natural graph edge:
--   Bill ──[stage, date, house]──▶ Sitting
-- Query all rows for a bill ordered by date to reconstruct its legislative journey.
CREATE TABLE IF NOT EXISTS bill_mentions (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    bill_id       UUID        NOT NULL REFERENCES bills(id)    ON DELETE CASCADE,
    sitting_id    UUID        NOT NULL REFERENCES sittings(id) ON DELETE CASCADE,
    house         TEXT        NOT NULL,
    date          DATE        NOT NULL,
    stage         TEXT,
    section_title TEXT        NOT NULL,
    speech_count  INT         NOT NULL DEFAULT 0,
    UNIQUE (bill_id, sitting_id, stage)
);

CREATE INDEX IF NOT EXISTS bill_mentions_bill_idx    ON bill_mentions (bill_id);
CREATE INDEX IF NOT EXISTS bill_mentions_sitting_idx ON bill_mentions (sitting_id);
CREATE INDEX IF NOT EXISTS bill_mentions_date_idx    ON bill_mentions (date);
