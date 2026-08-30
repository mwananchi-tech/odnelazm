ALTER TABLE sittings
    ADD COLUMN IF NOT EXISTS parliament TEXT;

CREATE INDEX IF NOT EXISTS sittings_parliament_idx ON sittings (parliament);

-- Existing single-parliament installations can be backfilled without assuming
-- which parliament they contain. Multi-parliament databases remain fail-closed.
WITH sole_parliament AS (
    SELECT min(parliament) AS parliament
    FROM members
    HAVING count(DISTINCT parliament) = 1
)
UPDATE sittings s
SET parliament = sole_parliament.parliament
FROM sole_parliament
WHERE s.parliament IS NULL;
