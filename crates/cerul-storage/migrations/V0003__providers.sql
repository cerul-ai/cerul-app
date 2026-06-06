CREATE TABLE providers (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    label TEXT NOT NULL,
    base_url TEXT,
    status TEXT NOT NULL DEFAULT 'unconfigured',
    last_error TEXT,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);

INSERT INTO providers (id, type, label, base_url, status)
VALUES ('local', 'local', 'Local on this Mac', NULL, 'ready');
