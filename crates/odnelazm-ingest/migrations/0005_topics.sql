-- Parliamentary topics: questions, statements, motions, and any other
-- subsection-level discussion that isn't a bill.
--
-- Each row represents one subsection from a sitting — a question asked,
-- a statement delivered, a motion debated, etc.

CREATE TABLE IF NOT EXISTS topics (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    sitting_id   UUID        NOT NULL REFERENCES sittings(id) ON DELETE CASCADE,
    section_type TEXT        NOT NULL,
    title        TEXT        NOT NULL,
    speech_count INT         NOT NULL DEFAULT 0,
    UNIQUE (sitting_id, section_type, title)
);

CREATE INDEX IF NOT EXISTS topics_sitting_idx ON topics (sitting_id);
CREATE INDEX IF NOT EXISTS topics_section_idx ON topics (section_type);
CREATE INDEX IF NOT EXISTS topics_title_trgm_idx ON topics USING gin (title gin_trgm_ops);

-- Which speakers contributed to a topic.
CREATE TABLE IF NOT EXISTS topic_speakers (
    topic_id     UUID NOT NULL REFERENCES topics(id)   ON DELETE CASCADE,
    speaker_id   UUID NOT NULL REFERENCES speakers(id) ON DELETE CASCADE,
    speech_count INT  NOT NULL DEFAULT 1,
    PRIMARY KEY (topic_id, speaker_id)
);

CREATE INDEX IF NOT EXISTS topic_speakers_speaker_idx ON topic_speakers (speaker_id);
