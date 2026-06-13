-- Indexes for the hottest query shapes:
--   * job worker polling claim_next_job every 2s per slot (status filter +
--     per-item correlated subquery),
--   * indexing_snapshot status counts,
--   * list_items ordered by indexed_at.
-- Without these every poll is a full table scan once the library grows.
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_item_type_status ON jobs(item_id, job_type, status);
CREATE INDEX IF NOT EXISTS idx_items_status ON items(status);
CREATE INDEX IF NOT EXISTS idx_items_indexed_at ON items(indexed_at);
CREATE INDEX IF NOT EXISTS idx_usage_events_item ON inference_usage_events(item_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_job ON inference_usage_events(job_id);
