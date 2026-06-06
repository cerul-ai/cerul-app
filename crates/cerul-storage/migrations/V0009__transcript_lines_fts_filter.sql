DROP TRIGGER IF EXISTS chunks_ai;
DROP TRIGGER IF EXISTS chunks_ad;
DROP TRIGGER IF EXISTS chunks_au;

DROP TABLE IF EXISTS chunks_fts;

CREATE VIRTUAL TABLE chunks_fts USING fts5(
    text,
    content='chunks',
    content_rowid='rowid'
);

INSERT INTO chunks_fts(rowid, text)
SELECT rowid, text
FROM chunks
WHERE text IS NOT NULL
  AND chunk_type IN ('transcript', 'audio', 'ocr', 'understanding');

CREATE TRIGGER chunks_ai AFTER INSERT ON chunks
WHEN new.text IS NOT NULL
 AND new.chunk_type IN ('transcript', 'audio', 'ocr', 'understanding')
BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER chunks_ad AFTER DELETE ON chunks
WHEN old.text IS NOT NULL
 AND old.chunk_type IN ('transcript', 'audio', 'ocr', 'understanding')
BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER chunks_au AFTER UPDATE OF text, chunk_type ON chunks
BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text)
    SELECT 'delete', old.rowid, old.text
    WHERE old.text IS NOT NULL
      AND old.chunk_type IN ('transcript', 'audio', 'ocr', 'understanding');

    INSERT INTO chunks_fts(rowid, text)
    SELECT new.rowid, new.text
    WHERE new.text IS NOT NULL
      AND new.chunk_type IN ('transcript', 'audio', 'ocr', 'understanding');
END;
