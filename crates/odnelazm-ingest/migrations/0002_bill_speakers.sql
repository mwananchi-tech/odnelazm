-- Per-bill-per-sitting discussion summary. Nullable; populated by an AI
-- enrichment pass after initial ingestion.
ALTER TABLE bill_mentions ADD COLUMN IF NOT EXISTS summary TEXT;

-- Which speakers contributed to a specific bill mention (i.e. spoke during
-- that bill's debate segment in that sitting). This makes it possible to ask:
--   "Who spoke about bill X in sitting Y?"
--   "Across all sittings, which members have debated bill X?"
CREATE TABLE IF NOT EXISTS bill_mention_speakers (
    bill_mention_id UUID NOT NULL REFERENCES bill_mentions(id) ON DELETE CASCADE,
    speaker_id      UUID NOT NULL REFERENCES speakers(id)      ON DELETE CASCADE,
    speech_count    INT  NOT NULL DEFAULT 1,
    PRIMARY KEY (bill_mention_id, speaker_id)
);

CREATE INDEX IF NOT EXISTS bill_mention_speakers_speaker_idx
    ON bill_mention_speakers (speaker_id);

CREATE INDEX IF NOT EXISTS bill_mention_speakers_mention_idx
    ON bill_mention_speakers (bill_mention_id);
