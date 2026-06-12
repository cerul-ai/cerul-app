CREATE TABLE moments (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    chunk_id TEXT REFERENCES chunks(id) ON DELETE SET NULL,
    start_sec REAL,
    end_sec REAL,
    title TEXT,
    quote TEXT NOT NULL,
    note TEXT,
    created_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE INDEX idx_moments_item ON moments(item_id);
CREATE INDEX idx_moments_created_at ON moments(created_at);
