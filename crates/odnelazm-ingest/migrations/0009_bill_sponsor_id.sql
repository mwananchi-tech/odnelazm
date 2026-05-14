ALTER TABLE bills ADD COLUMN IF NOT EXISTS sponsor_id UUID REFERENCES members(id);
CREATE INDEX IF NOT EXISTS bills_sponsor_id_idx ON bills (sponsor_id);

-- Link existing bills to member records via fuzzy name match.
-- Re-running is safe: only updates rows where sponsor_id is still NULL.
UPDATE bills b
SET sponsor_id = (
  SELECT id FROM match_member(b.sponsor, 0.4)
  ORDER BY score DESC
  LIMIT 1
)
WHERE b.sponsor IS NOT NULL
  AND b.sponsor_id IS NULL;
