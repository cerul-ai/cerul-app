CREATE TABLE inference_usage_events (
    id TEXT PRIMARY KEY,
    created_at INTEGER DEFAULT (strftime('%s','now')),
    provider_mode TEXT NOT NULL,
    capability TEXT NOT NULL,
    provider_id TEXT,
    provider_type TEXT,
    model_id TEXT,
    item_id TEXT REFERENCES items(id) ON DELETE SET NULL,
    job_id TEXT REFERENCES jobs(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'succeeded',
    request_count INTEGER NOT NULL DEFAULT 1,
    input_tokens INTEGER,
    output_tokens INTEGER,
    audio_seconds REAL,
    image_count INTEGER,
    video_seconds REAL,
    estimated_usd REAL,
    billed_credits REAL,
    price_snapshot_id TEXT,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_usage_events_created_at ON inference_usage_events(created_at);
CREATE INDEX idx_usage_events_item ON inference_usage_events(item_id);
CREATE INDEX idx_usage_events_job ON inference_usage_events(job_id);
CREATE INDEX idx_usage_events_mode ON inference_usage_events(provider_mode);
CREATE INDEX idx_usage_events_capability ON inference_usage_events(capability);
