-- Overall AI-generated summary of a bill's full legislative journey.
-- Populated by a separate enrichment pass; null until then.
ALTER TABLE bills ADD COLUMN IF NOT EXISTS summary TEXT;
