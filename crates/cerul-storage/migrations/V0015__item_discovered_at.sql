ALTER TABLE items ADD COLUMN discovered_at INTEGER;

UPDATE items
SET discovered_at = COALESCE(indexed_at, CAST(strftime('%s','now') AS INTEGER))
WHERE discovered_at IS NULL;

CREATE TRIGGER items_set_discovered_at
AFTER INSERT ON items
WHEN NEW.discovered_at IS NULL
BEGIN
    UPDATE items
    SET discovered_at = CAST(strftime('%s','now') AS INTEGER)
    WHERE id = NEW.id;
END;

CREATE INDEX idx_items_discovered_at ON items(discovered_at DESC);
