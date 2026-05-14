-- Store the actual text each speaker contributed to a bill debate or topic,
-- plus a nullable AI-generated summary populated by a separate enrichment pass.

ALTER TABLE bill_mention_speakers
    ADD COLUMN IF NOT EXISTS contributions_text TEXT,
    ADD COLUMN IF NOT EXISTS summary            TEXT;

ALTER TABLE topic_speakers
    ADD COLUMN IF NOT EXISTS contributions_text TEXT,
    ADD COLUMN IF NOT EXISTS summary            TEXT;
