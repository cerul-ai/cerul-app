CREATE TABLE embedding_profiles (
    id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    output_dimension INTEGER NOT NULL,
    distance_metric TEXT NOT NULL,
    instruction_template TEXT,
    index_version INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    updated_at INTEGER DEFAULT (strftime('%s','now'))
);

CREATE INDEX idx_embedding_profiles_status ON embedding_profiles(status);
