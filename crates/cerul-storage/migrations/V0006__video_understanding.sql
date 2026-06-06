CREATE TABLE item_understandings (
    item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    provider_id TEXT,
    model_id TEXT,
    status TEXT NOT NULL,
    summary TEXT,
    result TEXT NOT NULL DEFAULT '{}',
    error TEXT,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE INDEX idx_item_understandings_status ON item_understandings(status);
