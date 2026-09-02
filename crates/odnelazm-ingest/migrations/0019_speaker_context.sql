ALTER TABLE speakers
    ADD COLUMN IF NOT EXISTS house TEXT,
    ADD COLUMN IF NOT EXISTS parliament TEXT;

WITH speaker_context AS (
    SELECT ss.speaker_id, min(s.house) AS house, min(s.parliament) AS parliament
    FROM sitting_speakers ss
    JOIN sittings s ON s.id = ss.sitting_id
    WHERE s.parliament IS NOT NULL
    GROUP BY ss.speaker_id
    HAVING count(DISTINCT s.house) = 1
       AND count(DISTINCT s.parliament) = 1
)
UPDATE speakers sp
SET house = speaker_context.house,
    parliament = speaker_context.parliament
FROM speaker_context
WHERE sp.id = speaker_context.speaker_id
  AND sp.house IS NULL
  AND sp.parliament IS NULL;

ALTER TABLE speakers DROP CONSTRAINT IF EXISTS speakers_name_url_unique;
ALTER TABLE speakers DROP CONSTRAINT IF EXISTS speakers_name_url_context_unique;
ALTER TABLE speakers
    ADD CONSTRAINT speakers_name_url_context_unique
    UNIQUE NULLS NOT DISTINCT (name, url, house, parliament);

CREATE INDEX IF NOT EXISTS speakers_context_idx ON speakers (house, parliament);
