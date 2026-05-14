-- Speaker deduplication
--
-- Two classes of duplicates exist:
--
-- 1. Same person, different name variants — same profile URL, many name spellings.
--    For each URL group, we elect the most-referenced row as canonical, merge
--    speech counts into it, and delete the rest.
--
-- 2. Same name, null URL — the UNIQUE(name, url) constraint treats each NULL as
--    distinct, so "Hon. Kuria Kimani" can exist hundreds of times. We collapse
--    these to one row per distinct name and merge references.
--
-- After deduplication, the constraint is changed to NULLS NOT DISTINCT so future
-- inserts correctly upsert instead of inserting duplicates.

-- ── Step 1: URL-based deduplication ──────────────────────────────────────────

CREATE TEMP TABLE url_canonical AS
WITH ref_counts AS (
    SELECT speaker_id, count(*) AS refs
    FROM (
        SELECT speaker_id FROM sitting_speakers
        UNION ALL
        SELECT speaker_id FROM bill_mention_speakers
    ) r
    GROUP BY speaker_id
),
ranked AS (
    SELECT sp.id, sp.url,
           row_number() OVER (
               PARTITION BY sp.url
               ORDER BY coalesce(rc.refs, 0) DESC, sp.id
           ) AS rn
    FROM speakers sp
    LEFT JOIN ref_counts rc ON rc.speaker_id = sp.id
    WHERE sp.url IS NOT NULL
)
SELECT id AS canonical_id, url FROM ranked WHERE rn = 1;

-- Merge sitting_speakers into canonical rows
INSERT INTO sitting_speakers (sitting_id, speaker_id, speech_count)
SELECT ss.sitting_id, uc.canonical_id, ss.speech_count
FROM sitting_speakers ss
JOIN speakers sp ON sp.id = ss.speaker_id
JOIN url_canonical uc ON uc.url = sp.url
WHERE sp.id <> uc.canonical_id
ON CONFLICT (sitting_id, speaker_id)
    DO UPDATE SET speech_count = sitting_speakers.speech_count + EXCLUDED.speech_count;

DELETE FROM sitting_speakers ss
USING speakers sp
JOIN url_canonical uc ON uc.url = sp.url
WHERE ss.speaker_id = sp.id AND sp.id <> uc.canonical_id;

-- Merge bill_mention_speakers into canonical rows
INSERT INTO bill_mention_speakers (bill_mention_id, speaker_id, speech_count)
SELECT bms.bill_mention_id, uc.canonical_id, bms.speech_count
FROM bill_mention_speakers bms
JOIN speakers sp ON sp.id = bms.speaker_id
JOIN url_canonical uc ON uc.url = sp.url
WHERE sp.id <> uc.canonical_id
ON CONFLICT (bill_mention_id, speaker_id)
    DO UPDATE SET speech_count = bill_mention_speakers.speech_count + EXCLUDED.speech_count;

DELETE FROM bill_mention_speakers bms
USING speakers sp
JOIN url_canonical uc ON uc.url = sp.url
WHERE bms.speaker_id = sp.id AND sp.id <> uc.canonical_id;

-- Delete non-canonical URL-based speaker rows
DELETE FROM speakers sp
USING url_canonical uc
WHERE sp.url = uc.url AND sp.id <> uc.canonical_id;

-- ── Step 2: Null-URL deduplication (same name, many rows) ────────────────────

CREATE TEMP TABLE null_url_dupes AS
WITH ranked AS (
    SELECT id, name,
           row_number() OVER (PARTITION BY name ORDER BY id) AS rn
    FROM speakers
    WHERE url IS NULL
)
SELECT id FROM ranked WHERE rn > 1;

CREATE TEMP TABLE null_url_canonical AS
SELECT DISTINCT ON (name) id AS canonical_id, name
FROM speakers
WHERE url IS NULL
ORDER BY name, id;

INSERT INTO sitting_speakers (sitting_id, speaker_id, speech_count)
SELECT ss.sitting_id, nuc.canonical_id, ss.speech_count
FROM sitting_speakers ss
JOIN speakers sp ON sp.id = ss.speaker_id
JOIN null_url_canonical nuc ON nuc.name = sp.name
WHERE sp.url IS NULL AND sp.id <> nuc.canonical_id
ON CONFLICT (sitting_id, speaker_id)
    DO UPDATE SET speech_count = sitting_speakers.speech_count + EXCLUDED.speech_count;

DELETE FROM sitting_speakers ss
USING speakers sp
JOIN null_url_canonical nuc ON nuc.name = sp.name
WHERE ss.speaker_id = sp.id
  AND sp.url IS NULL
  AND sp.id <> nuc.canonical_id;

INSERT INTO bill_mention_speakers (bill_mention_id, speaker_id, speech_count)
SELECT bms.bill_mention_id, nuc.canonical_id, bms.speech_count
FROM bill_mention_speakers bms
JOIN speakers sp ON sp.id = bms.speaker_id
JOIN null_url_canonical nuc ON nuc.name = sp.name
WHERE sp.url IS NULL AND sp.id <> nuc.canonical_id
ON CONFLICT (bill_mention_id, speaker_id)
    DO UPDATE SET speech_count = bill_mention_speakers.speech_count + EXCLUDED.speech_count;

DELETE FROM bill_mention_speakers bms
USING speakers sp
JOIN null_url_canonical nuc ON nuc.name = sp.name
WHERE bms.speaker_id = sp.id
  AND sp.url IS NULL
  AND sp.id <> nuc.canonical_id;

DELETE FROM speakers WHERE id IN (SELECT id FROM null_url_dupes);

-- ── Step 3: Fix unique constraint to prevent future null duplicates ───────────

ALTER TABLE speakers DROP CONSTRAINT IF EXISTS speakers_name_url_key;
ALTER TABLE speakers ADD CONSTRAINT speakers_name_url_unique
    UNIQUE NULLS NOT DISTINCT (name, url);
