-- Reconciliation metadata for sitting-scoped projections. Rows disappear from
-- a refreshed extraction by becoming inactive, not by being deleted.
ALTER TABLE sitting_speakers
    ADD COLUMN IF NOT EXISTS active        BOOLEAN     NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS input_hash    TEXT;

ALTER TABLE bill_mentions
    ADD COLUMN IF NOT EXISTS active        BOOLEAN     NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS input_hash    TEXT;

ALTER TABLE bill_mention_speakers
    ADD COLUMN IF NOT EXISTS active        BOOLEAN     NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS input_hash    TEXT;

ALTER TABLE topics
    ADD COLUMN IF NOT EXISTS active        BOOLEAN     NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS input_hash    TEXT;

ALTER TABLE topic_speakers
    ADD COLUMN IF NOT EXISTS active        BOOLEAN     NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS input_hash    TEXT;

-- These formulas are also used by reconciliation. Backfilling prevents an
-- unchanged first refresh from making existing summaries appear stale.
UPDATE sitting_speakers
SET input_hash = md5(speech_count::text)
WHERE input_hash IS NULL;

UPDATE bill_mention_speakers
SET input_hash = md5(speech_count::text || chr(31) || COALESCE(contributions_text, ''))
WHERE input_hash IS NULL;

UPDATE topic_speakers
SET input_hash = md5(speech_count::text || chr(31) || COALESCE(contributions_text, ''))
WHERE input_hash IS NULL;

WITH hashes AS (
    SELECT bm.id,
           md5(bm.bill_id::text || chr(31) || COALESCE(bm.stage, '<NULL>') || chr(31) ||
               bm.section_title || chr(31) || bm.speech_count::text || chr(31) ||
               COALESCE(string_agg(bms.speaker_id::text || ':' || bms.input_hash, ','
                                   ORDER BY bms.speaker_id) FILTER (WHERE bms.active), '')) AS input_hash
    FROM bill_mentions bm
    LEFT JOIN bill_mention_speakers bms ON bms.bill_mention_id = bm.id
    GROUP BY bm.id
)
UPDATE bill_mentions bm SET input_hash = hashes.input_hash
FROM hashes WHERE bm.id = hashes.id AND bm.input_hash IS NULL;

WITH hashes AS (
    SELECT t.id,
           md5(t.section_type || chr(31) || t.title || chr(31) || t.speech_count::text || chr(31) ||
               COALESCE(string_agg(ts.speaker_id::text || ':' || ts.input_hash, ','
                                   ORDER BY ts.speaker_id) FILTER (WHERE ts.active), '')) AS input_hash
    FROM topics t
    LEFT JOIN topic_speakers ts ON ts.topic_id = t.id
    GROUP BY t.id
)
UPDATE topics t SET input_hash = hashes.input_hash
FROM hashes WHERE t.id = hashes.id AND t.input_hash IS NULL;

WITH hashes AS (
    SELECT b.id,
           md5(COALESCE(string_agg(bm.id::text || ':' || bm.input_hash, ','
                                   ORDER BY bm.date, bm.id) FILTER (WHERE bm.active), '')) AS input_hash
    FROM bills b
    LEFT JOIN bill_mentions bm ON bm.bill_id = b.id
    GROUP BY b.id
)
UPDATE bills b SET summary_input_hash = COALESCE(b.summary_input_hash, hashes.input_hash)
FROM hashes WHERE b.id = hashes.id;

UPDATE bill_mentions
SET summary_input_hash = COALESCE(summary_input_hash, input_hash)
WHERE summary IS NOT NULL;

UPDATE bill_mention_speakers
SET summary_input_hash = COALESCE(summary_input_hash, input_hash)
WHERE summary IS NOT NULL;

UPDATE topics
SET summary_input_hash = COALESCE(summary_input_hash, input_hash)
WHERE summary IS NOT NULL;

UPDATE topic_speakers
SET summary_input_hash = COALESCE(summary_input_hash, input_hash)
WHERE summary IS NOT NULL;

UPDATE sittings
SET generated_summary_input_hash = COALESCE(generated_summary_input_hash, md5(raw_json::text))
WHERE generated_summary IS NOT NULL;

CREATE INDEX IF NOT EXISTS sitting_speakers_active_idx
    ON sitting_speakers (sitting_id) WHERE active;
CREATE INDEX IF NOT EXISTS bill_mentions_active_idx
    ON bill_mentions (sitting_id) WHERE active;
CREATE INDEX IF NOT EXISTS topics_active_idx
    ON topics (sitting_id) WHERE active;
