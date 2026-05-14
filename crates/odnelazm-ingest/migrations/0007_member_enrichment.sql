-- Additional profile data fetched from individual member profile pages.
-- Populated by a separate enrichment pass after the member listing is imported.

ALTER TABLE members
    ADD COLUMN IF NOT EXISTS photo_url          TEXT,
    ADD COLUMN IF NOT EXISTS biography          TEXT,
    ADD COLUMN IF NOT EXISTS party              TEXT,
    ADD COLUMN IF NOT EXISTS positions          JSONB,
    ADD COLUMN IF NOT EXISTS committees         JSONB,
    ADD COLUMN IF NOT EXISTS speeches_last_year INT,
    ADD COLUMN IF NOT EXISTS speeches_total     INT,
    ADD COLUMN IF NOT EXISTS bills_total        INT;
