-- Source registry and ingestion audit trail. Fixed identifiers keep built-in
-- sources deterministic without depending on database extension schemas.
CREATE TABLE IF NOT EXISTS data_sources (
    id         UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    source_key TEXT        NOT NULL UNIQUE,
    name       TEXT        NOT NULL,
    base_url   TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO data_sources (id, source_key, name, base_url)
VALUES
    ('00000000-0000-5000-8000-000000000001',
     'mzalendo-current', 'Mzalendo current', 'https://mzalendo.com'),
    ('00000000-0000-5000-8000-000000000002',
     'mzalendo-archive', 'Mzalendo archive', 'https://info.mzalendo.com')
ON CONFLICT (source_key) DO UPDATE SET
    name       = EXCLUDED.name,
    base_url   = EXCLUDED.base_url,
    updated_at = now();

CREATE TABLE IF NOT EXISTS ingestion_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    data_source_id   UUID        NOT NULL REFERENCES data_sources(id) ON DELETE RESTRICT,
    status           TEXT        NOT NULL DEFAULT 'pending',
    discovered_count BIGINT      NOT NULL DEFAULT 0,
    processed_count  BIGINT      NOT NULL DEFAULT 0,
    succeeded_count  BIGINT      NOT NULL DEFAULT 0,
    skipped_count    BIGINT      NOT NULL DEFAULT 0,
    failed_count     BIGINT      NOT NULL DEFAULT 0,
    error_message    TEXT,
    error_metadata   JSONB,
    queued_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at       TIMESTAMPTZ,
    finished_at      TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ingestion_runs_status_check CHECK (
        status IN ('pending', 'running', 'succeeded', 'partial', 'failed', 'cancelled')
    ),
    CONSTRAINT ingestion_runs_counts_check CHECK (
        discovered_count >= 0 AND processed_count >= 0 AND
        succeeded_count >= 0 AND skipped_count >= 0 AND failed_count >= 0
    ),
    CONSTRAINT ingestion_runs_timestamps_check CHECK (
        (started_at IS NULL OR started_at >= queued_at) AND
        (finished_at IS NULL OR (started_at IS NOT NULL AND finished_at >= started_at))
    )
);

CREATE INDEX IF NOT EXISTS ingestion_runs_data_source_idx
    ON ingestion_runs (data_source_id);
CREATE INDEX IF NOT EXISTS ingestion_runs_status_queued_idx
    ON ingestion_runs (status, queued_at);

-- Source-specific identifiers remain separate from canonical records so future
-- sources and aliases can be added without rewriting canonical data.
CREATE TABLE IF NOT EXISTS sitting_sources (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    sitting_id     UUID        NOT NULL REFERENCES sittings(id) ON DELETE CASCADE,
    data_source_id UUID        NOT NULL REFERENCES data_sources(id) ON DELETE RESTRICT,
    external_key   TEXT,
    source_url     TEXT        NOT NULL,
    normalized_url TEXT,
    payload_hash   TEXT,
    raw_metadata   JSONB       NOT NULL DEFAULT '{}'::jsonb,
    first_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT sitting_sources_seen_at_check CHECK (last_seen_at >= first_seen_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS sitting_sources_external_key_unique
    ON sitting_sources (data_source_id, external_key)
    WHERE external_key IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS sitting_sources_normalized_url_unique
    ON sitting_sources (data_source_id, normalized_url)
    WHERE normalized_url IS NOT NULL;
CREATE INDEX IF NOT EXISTS sitting_sources_sitting_idx
    ON sitting_sources (sitting_id);
CREATE INDEX IF NOT EXISTS sitting_sources_data_source_idx
    ON sitting_sources (data_source_id);

CREATE TABLE IF NOT EXISTS member_sources (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    member_id      UUID        NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    data_source_id UUID        NOT NULL REFERENCES data_sources(id) ON DELETE RESTRICT,
    external_key   TEXT,
    source_url     TEXT        NOT NULL,
    normalized_url TEXT,
    payload_hash   TEXT,
    raw_metadata   JSONB       NOT NULL DEFAULT '{}'::jsonb,
    first_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT member_sources_seen_at_check CHECK (last_seen_at >= first_seen_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS member_sources_external_key_unique
    ON member_sources (data_source_id, external_key)
    WHERE external_key IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS member_sources_normalized_url_unique
    ON member_sources (data_source_id, normalized_url)
    WHERE normalized_url IS NOT NULL;
CREATE INDEX IF NOT EXISTS member_sources_member_idx
    ON member_sources (member_id);
CREATE INDEX IF NOT EXISTS member_sources_data_source_idx
    ON member_sources (data_source_id);

-- Existing canonical URLs seed source provenance. Generic ON CONFLICT handles
-- both reruns and canonical rows whose URLs normalize to the same source URL.
INSERT INTO sitting_sources (
    sitting_id, data_source_id, external_key, source_url, normalized_url, raw_metadata,
    first_seen_at, last_seen_at
)
SELECT
    s.id,
    ds.id,
    CASE WHEN ds.source_key = 'mzalendo-current'
        THEN substring(regexp_replace(s.url, '/+$', '') FROM '([0-9]+)$')
    END,
    s.url,
    regexp_replace(s.url, '/+$', ''),
    jsonb_strip_nulls(jsonb_build_object(
        'house', s.house,
        'date', s.date,
        'session_type', s.session_type,
        'source', s.source
    )),
    s.ingested_at,
    s.ingested_at
FROM sittings s
JOIN data_sources ds ON ds.source_key = CASE
    WHEN s.url LIKE 'https://info.mzalendo.com/%'
      OR s.source = 'https://info.mzalendo.com'
        THEN 'mzalendo-archive'
    WHEN s.url LIKE 'https://mzalendo.com/%'
      OR s.source = 'https://mzalendo.com'
        THEN 'mzalendo-current'
END
ON CONFLICT DO NOTHING;

UPDATE sitting_sources ss
SET external_key = substring(regexp_replace(ss.source_url, '/+$', '') FROM '([0-9]+)$')
FROM data_sources ds
WHERE ss.data_source_id = ds.id
  AND ds.source_key = 'mzalendo-current'
  AND ss.external_key IS NULL;

WITH member_source_rows AS (
    SELECT
        m.*,
        ds.id AS data_source_id,
        regexp_replace(
            regexp_replace(
                regexp_replace(
                    regexp_replace(
                        CASE
                            WHEN m.url ~ '^https?://' THEN m.url
                            WHEN ds.source_key = 'mzalendo-archive'
                                THEN 'https://info.mzalendo.com/' || ltrim(m.url, '/')
                            ELSE 'https://mzalendo.com/' || ltrim(m.url, '/')
                        END,
                        '^https?://(www\.)?mzalendo\.com', 'https://mzalendo.com', 'i'),
                    '^https?://(www\.)?info\.mzalendo\.com', 'https://info.mzalendo.com', 'i'),
                '[?#].*$', ''),
            '/+$', '') AS normalized_url
    FROM members m
    JOIN data_sources ds ON ds.source_key = CASE
        WHEN m.url ~* '^https?://(www\.)?info\.mzalendo\.com/'
            THEN 'mzalendo-archive'
        WHEN m.url ~* '^https?://(www\.)?mzalendo\.com/' OR m.url !~ '^https?://'
            THEN 'mzalendo-current'
    END
)
INSERT INTO member_sources (
    member_id, data_source_id, external_key, source_url, normalized_url, raw_metadata
)
SELECT
    m.id,
    m.data_source_id,
    CASE
        WHEN m.normalized_url ~ '/person/[^/]+$'
          OR m.normalized_url ~ '/mps-performance/[^/]+/[^/]+/[^/]+$'
            THEN substring(m.normalized_url FROM '/([^/]+)$')
    END,
    m.url,
    m.normalized_url,
    jsonb_strip_nulls(jsonb_build_object(
        'house', m.house,
        'parliament', m.parliament,
        'role', m.role,
        'constituency', m.constituency
    ))
FROM member_source_rows m
ON CONFLICT DO NOTHING;

-- Lifecycle metadata for each independently generated summary. Sitting columns
-- are prefixed to distinguish generated_summary from the source-provided summary.
ALTER TABLE sittings
    ADD COLUMN IF NOT EXISTS generated_summary_input_hash     TEXT,
    ADD COLUMN IF NOT EXISTS generated_summary_prompt_version TEXT,
    ADD COLUMN IF NOT EXISTS generated_summary_generated_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS generated_summary_stale_at       TIMESTAMPTZ;

ALTER TABLE bills
    ADD COLUMN IF NOT EXISTS summary_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS summary_prompt_version  TEXT,
    ADD COLUMN IF NOT EXISTS summary_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS summary_stale_at        TIMESTAMPTZ;

ALTER TABLE bill_mentions
    ADD COLUMN IF NOT EXISTS summary_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS summary_prompt_version  TEXT,
    ADD COLUMN IF NOT EXISTS summary_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS summary_stale_at        TIMESTAMPTZ;

ALTER TABLE bill_mention_speakers
    ADD COLUMN IF NOT EXISTS summary_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS summary_prompt_version  TEXT,
    ADD COLUMN IF NOT EXISTS summary_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS summary_stale_at        TIMESTAMPTZ;

ALTER TABLE topics
    ADD COLUMN IF NOT EXISTS summary_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS summary_prompt_version  TEXT,
    ADD COLUMN IF NOT EXISTS summary_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS summary_stale_at        TIMESTAMPTZ;

ALTER TABLE topic_speakers
    ADD COLUMN IF NOT EXISTS summary_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS summary_prompt_version  TEXT,
    ADD COLUMN IF NOT EXISTS summary_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS summary_stale_at        TIMESTAMPTZ;
