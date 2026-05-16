ALTER TABLE topics
    ADD COLUMN IF NOT EXISTS summary       text,
    ADD COLUMN IF NOT EXISTS summary_model text;
