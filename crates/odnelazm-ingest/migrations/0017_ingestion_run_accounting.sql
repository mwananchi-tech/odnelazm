-- Every discovered item must have exactly one terminal outcome. This makes a
-- completed run usable as a completeness assertion rather than a loose metric.
ALTER TABLE ingestion_runs
    DROP CONSTRAINT IF EXISTS ingestion_runs_accounting_check;

ALTER TABLE ingestion_runs
    ADD CONSTRAINT ingestion_runs_accounting_check CHECK (
        processed_count = succeeded_count + skipped_count + failed_count
        AND processed_count <= discovered_count
    );
