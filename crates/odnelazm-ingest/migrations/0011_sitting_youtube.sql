-- YouTube URL for the live stream or archived recording of a sitting.
-- Populated manually or via a future enrichment step.
ALTER TABLE sittings ADD COLUMN IF NOT EXISTS youtube_url TEXT;
