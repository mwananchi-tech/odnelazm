-- Replace the UNIQUE constraint on bill_mentions so that multiple NULL stages
-- for the same (bill, sitting) pair are treated as duplicates, not distinct rows.
ALTER TABLE bill_mentions
  DROP CONSTRAINT IF EXISTS bill_mentions_bill_id_sitting_id_stage_key;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'bill_mentions_bill_id_sitting_id_stage_unique'
  ) THEN
    ALTER TABLE bill_mentions
      ADD CONSTRAINT bill_mentions_bill_id_sitting_id_stage_unique
      UNIQUE NULLS NOT DISTINCT (bill_id, sitting_id, stage);
  END IF;
END$$;
