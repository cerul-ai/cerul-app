CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    config TEXT NOT NULL,
    status TEXT NOT NULL,
    last_poll_at INTEGER,
    created_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE TABLE items (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    content_type TEXT NOT NULL,
    external_id TEXT,
    title TEXT,
    duration_sec REAL,
    raw_path TEXT,
    indexed_at INTEGER,
    status TEXT NOT NULL,
    error TEXT,
    metadata TEXT,
    UNIQUE(source_id, external_id)
);

CREATE TABLE chunks (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    chunk_type TEXT NOT NULL,
    start_sec REAL,
    end_sec REAL,
    text TEXT,
    frame_path TEXT,
    metadata TEXT
);

CREATE INDEX idx_chunks_item ON chunks(item_id);
CREATE INDEX idx_chunks_type ON chunks(chunk_type);

CREATE VIRTUAL TABLE chunks_fts USING fts5(
    text,
    content='chunks',
    content_rowid='rowid'
);

CREATE TRIGGER chunks_ai AFTER INSERT ON chunks WHEN new.text IS NOT NULL BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER chunks_ad AFTER DELETE ON chunks WHEN old.text IS NOT NULL BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER chunks_au AFTER UPDATE OF text ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text)
    SELECT 'delete', old.rowid, old.text
    WHERE old.text IS NOT NULL;

    INSERT INTO chunks_fts(rowid, text)
    SELECT new.rowid, new.text
    WHERE new.text IS NOT NULL;
END;

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    item_id TEXT REFERENCES items(id) ON DELETE CASCADE,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    error TEXT,
    progress REAL DEFAULT 0
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);
