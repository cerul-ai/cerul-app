-- Cerul Cloud token mode was removed (see "Remove Cerul Cloud token mode").
-- Clean up settings left behind on databases upgraded from versions that still
-- shipped the cloud flow: the stored token must no longer be retained or
-- exposed via /settings, and removed "cloud"/"byok" inference modes collapse
-- into "remote". Both statements are no-ops on databases that never had the flow.

DELETE FROM settings
WHERE key IN (
    'cloud_api_key',
    'cloud_connected',
    'cloud_account_email',
    'cloud_email',
    'cloud_plan',
    'cloud_quota_percent'
);

UPDATE settings
SET value = '"remote"', updated_at = strftime('%s','now')
WHERE key = 'inference_mode' AND value IN ('"cloud"', '"byok"');
