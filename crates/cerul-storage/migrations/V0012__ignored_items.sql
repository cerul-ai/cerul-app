CREATE TABLE ignored_items (
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    ignored_at INTEGER DEFAULT (strftime('%s','now')),
    reason TEXT,
    PRIMARY KEY (source_id, external_id)
);

CREATE INDEX idx_ignored_items_source ON ignored_items(source_id);
