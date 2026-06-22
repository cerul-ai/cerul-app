CREATE TABLE retrieval_units (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    unit_index INTEGER NOT NULL,
    unit_kind TEXT NOT NULL,
    start_sec REAL,
    end_sec REAL,
    content_text TEXT NOT NULL,
    transcript_text TEXT,
    ocr_text TEXT,
    visual_text TEXT,
    summary_text TEXT,
    representative_chunk_id TEXT,
    representative_frame_path TEXT,
    embedding_profile_id TEXT NOT NULL,
    index_version INTEGER NOT NULL,
    metadata TEXT,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    updated_at INTEGER DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, index_version, embedding_profile_id, unit_index)
);

CREATE INDEX idx_retrieval_units_item ON retrieval_units(item_id);
CREATE INDEX idx_retrieval_units_profile_version ON retrieval_units(embedding_profile_id, index_version);
CREATE INDEX idx_retrieval_units_kind ON retrieval_units(unit_kind);
CREATE INDEX idx_retrieval_units_time ON retrieval_units(item_id, start_sec, end_sec);

CREATE VIRTUAL TABLE retrieval_units_fts USING fts5(
    content_text,
    content='retrieval_units',
    content_rowid='rowid'
);

CREATE TRIGGER retrieval_units_ai AFTER INSERT ON retrieval_units WHEN new.content_text IS NOT NULL BEGIN
    INSERT INTO retrieval_units_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
END;

CREATE TRIGGER retrieval_units_ad AFTER DELETE ON retrieval_units WHEN old.content_text IS NOT NULL BEGIN
    INSERT INTO retrieval_units_fts(retrieval_units_fts, rowid, content_text)
    VALUES ('delete', old.rowid, old.content_text);
END;

CREATE TRIGGER retrieval_units_au AFTER UPDATE OF content_text ON retrieval_units BEGIN
    INSERT INTO retrieval_units_fts(retrieval_units_fts, rowid, content_text)
    SELECT 'delete', old.rowid, old.content_text
    WHERE old.content_text IS NOT NULL;

    INSERT INTO retrieval_units_fts(rowid, content_text)
    SELECT new.rowid, new.content_text
    WHERE new.content_text IS NOT NULL;
END;

ALTER TABLE items ADD COLUMN search_index_version INTEGER;
ALTER TABLE items ADD COLUMN search_index_status TEXT;
ALTER TABLE items ADD COLUMN search_index_error TEXT;
ALTER TABLE items ADD COLUMN search_index_unit_count INTEGER DEFAULT 0;
ALTER TABLE items ADD COLUMN search_index_vector_count INTEGER DEFAULT 0;
