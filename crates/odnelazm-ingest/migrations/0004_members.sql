CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ── Members ───────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS members (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name         TEXT        NOT NULL,
    url          TEXT        NOT NULL UNIQUE,
    house        TEXT        NOT NULL,
    parliament   TEXT        NOT NULL DEFAULT '13th-parliament',
    role         TEXT,
    constituency TEXT
);

-- GIN index for fast trigram search (used by match_member)
CREATE INDEX IF NOT EXISTS members_name_trgm_idx ON members USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS members_url_idx       ON members (url);

-- ── Speaker → Member link ─────────────────────────────────────────────────────

ALTER TABLE speakers
    ADD COLUMN IF NOT EXISTS member_id UUID REFERENCES members(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS speakers_member_id_idx ON speakers (member_id);

-- ── Helper: clean a raw speaker name into a matchable core name ───────────────
--
-- Handles patterns like:
--   "Hon. Kimani Kuria (Molo, UDA)"         → "Kimani Kuria"
--   "The Temporary Speaker (Hon. P. Kaluma)" → "P. Kaluma"
--   "Sen. (Prof.) Lonyangapuo"               → "Lonyangapuo"

CREATE OR REPLACE FUNCTION clean_speaker_name(raw TEXT)
RETURNS TEXT
LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
    regexp_replace(
        trim(raw),
        -- 1. Strip presiding-role prefix: "The/Hon. (Temporary) (Deputy) Speaker/Chairman ..."
        '^(The|Hon\.?)\s+(Temporary\s+)?(Deputy\s+)?(Speaker|Chairman|Chairlady|Chairperson|Chair|Deputy\s+Speaker)\s*',
        '', 'i'),
        -- 2. Unwrap lone outer parenthetical (greedy to handle nested parens like "(Hon. (Dr) X)")
        '^\s*\((.+)\)\s*$', '\1'),
        -- 3. Strip honorifics
        '\m(Hon|Sen|Mr|Mrs|Ms|Madam|Mhe)\.?\s*', '', 'gi'),
        -- 4. Strip academic/professional titles (standalone or in parens)
        '\(?(Dr|Prof|Eng|Rtd|Ret|SC|EGH|CBS|OGW|MBS|EBS|HSC|OBS|Capt|Maj|Col|Gen|Lt|Cdr)\.?\)?\s*', '', 'gi'),
        -- 5. Strip constituency / party parenthetical at end: "(Molo, UDA)"
        '\s*\([^)]*\)\s*$', ''),
        -- 5b. Strip unclosed parenthetical at end: "(Kikuyu, UDA" (missing closing paren)
        '\s*\([^)]*$', ''),
        -- 6. Strip trailing noise: stray ): ; chars
        '[:\);,]+\s*$', ''),
        -- 7. Strip possessives
        '''s?\s', ' '),
        -- 8. Collapse whitespace
        '\s+', ' ')
$$;

-- ── Core function: match a raw speaker name to members ────────────────────────
--
-- Strategy (in order of preference):
--   1. word_similarity on cleaned name  — handles abbreviated first/last names
--   2. plain similarity on cleaned name — fallback
--
-- word_similarity(a, b) = max similarity between a and any trigram substring of b,
-- making it ideal when the query is a short form of the full member name.

CREATE OR REPLACE FUNCTION match_member(
    query_name TEXT,
    min_score  FLOAT DEFAULT 0.3
)
RETURNS TABLE (
    id           UUID,
    name         TEXT,
    url          TEXT,
    house        TEXT,
    constituency TEXT,
    score        FLOAT
)
LANGUAGE sql STABLE AS $$
    WITH cleaned AS (
        SELECT clean_speaker_name(query_name) AS cn
    )
    SELECT
        m.id,
        m.name,
        m.url,
        m.house,
        m.constituency,
        greatest(
            word_similarity(c.cn, m.name),
            similarity(c.cn, m.name)
        )::FLOAT AS score
    FROM members m, cleaned c
    WHERE greatest(
              word_similarity(c.cn, m.name),
              similarity(c.cn, m.name)
          ) >= min_score
    ORDER BY score DESC
    LIMIT 5
$$;

-- ── Linkage: wire speakers to members ────────────────────────────────────────
--
-- Step 1 — exact URL match (run immediately; re-runs are safe via IS NULL guard)
-- Called by the pipeline after importing members.

CREATE OR REPLACE FUNCTION link_speakers_by_url()
RETURNS BIGINT
LANGUAGE sql AS $$
    WITH updated AS (
        UPDATE speakers sp
        SET    member_id = m.id
        FROM   members m
        WHERE  sp.url = m.url
          AND  sp.member_id IS NULL
        RETURNING 1
    )
    SELECT count(*)::BIGINT FROM updated
$$;

-- Step 2 — name-based fuzzy match for speakers without a URL
-- Only matches where the best score is ≥ 0.45 (tuned to reduce false positives).

CREATE OR REPLACE FUNCTION link_speakers_by_name(min_score FLOAT DEFAULT 0.45)
RETURNS BIGINT
LANGUAGE plpgsql AS $$
DECLARE
    linked BIGINT := 0;
BEGIN
    WITH best_match AS (
        SELECT
            sp.id AS speaker_id,
            (SELECT mm.id
             FROM match_member(sp.name, min_score) mm
             ORDER BY score DESC
             LIMIT 1) AS member_id
        FROM speakers sp
        WHERE sp.url IS NULL
          AND sp.member_id IS NULL
          AND length(sp.name) > 5
    )
    UPDATE speakers sp
    SET    member_id = bm.member_id
    FROM   best_match bm
    WHERE  sp.id = bm.speaker_id
      AND  bm.member_id IS NOT NULL;

    GET DIAGNOSTICS linked = ROW_COUNT;
    RETURN linked;
END
$$;
