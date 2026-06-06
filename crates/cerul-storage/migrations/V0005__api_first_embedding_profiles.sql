ALTER TABLE embedding_profiles ADD COLUMN provider_id TEXT NOT NULL DEFAULT 'local';

UPDATE embedding_profiles
SET status = 'archived',
    updated_at = strftime('%s','now')
WHERE id IN ('qwen3-vl-embedding-2b-2048', 'qwen3-vl-2b-2048')
  AND status = 'active';
