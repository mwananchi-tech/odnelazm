-- Track which model generated each AI summary.
ALTER TABLE bill_mention_speakers ADD COLUMN IF NOT EXISTS summary_model TEXT;
ALTER TABLE topic_speakers        ADD COLUMN IF NOT EXISTS summary_model TEXT;
ALTER TABLE bill_mentions         ADD COLUMN IF NOT EXISTS summary_model TEXT;
ALTER TABLE bills                 ADD COLUMN IF NOT EXISTS summary_model TEXT;
ALTER TABLE sittings              ADD COLUMN IF NOT EXISTS generated_summary_model TEXT;
