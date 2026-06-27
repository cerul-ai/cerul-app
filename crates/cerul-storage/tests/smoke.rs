use cerul_storage::{sqlite, vectors, AppPaths, StorageTranscriptChunk, StorageTranscriptLine};

#[tokio::test]
async fn smoke_db_initializes() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();

    let conn = sqlite::open(&paths).unwrap();
    let objects: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
        .unwrap();
    assert!(objects > 0);

    let source_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(source_count, 0);

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks_fts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fts_count, 0);

    let profile = vectors::ensure_active_embedding_profile(&paths).unwrap();
    let collections = vectors::collection_names(&paths, &profile);
    vectors::ensure_collections(&paths).await.unwrap();
    assert_eq!(
        vectors::collection_point_count(&paths, &collections.text)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        vectors::collection_point_count(&paths, &collections.image)
            .await
            .unwrap(),
        0
    );
}

#[test]
fn legacy_default_embedding_profile_is_canonicalized() {
    assert_legacy_embedding_profile_is_canonicalized(vectors::LEGACY_DEFAULT_EMBEDDING_PROFILE_ID);
    assert_legacy_embedding_profile_is_canonicalized(vectors::LEGACY_QWEN3_EMBEDDING_PROFILE_ID);
}

fn assert_legacy_embedding_profile_is_canonicalized(legacy_profile_id: &str) {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let conn = sqlite::open(&paths).unwrap();

    conn.execute(
        r#"
        INSERT INTO embedding_profiles (
            id, model_id, output_dimension, distance_metric, index_version, status
        )
        VALUES (?1, ?2, 2048, 'cosine', 2, 'active')
        "#,
        (legacy_profile_id, vectors::DEFAULT_EMBEDDING_MODEL_ID),
    )
    .unwrap();
    conn.execute(
        r#"
        INSERT INTO settings (key, value)
        VALUES ('active_embedding_profile', ?1)
        "#,
        [serde_json::Value::String(legacy_profile_id.to_string()).to_string()],
    )
    .unwrap();

    let profile = vectors::ensure_active_embedding_profile(&paths).unwrap();
    let selected: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'active_embedding_profile'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_status: String = conn
        .query_row(
            "SELECT status FROM embedding_profiles WHERE id = ?1",
            [legacy_profile_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(profile.id, vectors::DEFAULT_EMBEDDING_PROFILE_ID);
    assert_eq!(
        selected,
        serde_json::Value::String(vectors::DEFAULT_EMBEDDING_PROFILE_ID.to_string()).to_string()
    );
    assert_eq!(legacy_status, "archived");
}

#[tokio::test]
async fn smoke_fts_tracks_chunk_text_changes() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let conn = sqlite::open(&paths).unwrap();

    conn.execute(
        "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO items (id, source_id, content_type, status) VALUES ('item-1', 'source-1', 'video', 'ready')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks (id, item_id, chunk_type, text) VALUES ('chunk-1', 'item-1', 'transcript', 'hello searchable world')",
        [],
    )
    .unwrap();

    assert_eq!(match_count(&conn, "hello"), 1);

    conn.execute("UPDATE chunks SET text = NULL WHERE id = 'chunk-1'", [])
        .unwrap();
    assert_eq!(match_count(&conn, "hello"), 0);

    conn.execute(
        "UPDATE chunks SET text = 'alpha searchable text' WHERE id = 'chunk-1'",
        [],
    )
    .unwrap();
    assert_eq!(match_count(&conn, "alpha"), 1);

    conn.execute("DELETE FROM chunks WHERE id = 'chunk-1'", [])
        .unwrap();
    assert_eq!(match_count(&conn, "alpha"), 0);
}

#[tokio::test]
async fn smoke_write_video_chunks_persists_sqlite_and_vector_index_vectors() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let conn = sqlite::open(&paths).unwrap();

    conn.execute(
        "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO items (id, source_id, content_type, status) VALUES ('item-1', 'source-1', 'video', 'ready')",
        [],
    )
    .unwrap();

    let frames_dir = temp.path().join("frames");
    std::fs::create_dir(&frames_dir).unwrap();
    let frame_a = frames_dir.join("frame_000001.jpg");
    let frame_b = frames_dir.join("frame_000002.jpg");
    std::fs::write(&frame_a, b"fake").unwrap();
    std::fs::write(&frame_b, b"fake").unwrap();
    let chunks = vec![
        StorageTranscriptChunk {
            start: 0.0,
            end: 30.0,
            text: "hello searchable vector".to_string(),
        },
        StorageTranscriptChunk {
            start: 25.0,
            end: 55.0,
            text: "overlap window".to_string(),
        },
    ];

    let summary = cerul_storage::write_video_chunks(
        &paths,
        "item-1",
        &chunks,
        &[frame_a, frame_b],
        &[fake_vector(0), fake_vector(1)],
        &[fake_vector(2), fake_vector(3)],
    )
    .await
    .unwrap();

    assert_eq!(summary.transcript_chunks, 2);
    assert_eq!(summary.keyframes, 2);
    assert_eq!(match_count(&conn, "searchable"), 1);

    let sqlite_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sqlite_chunks, 4);

    let profile = vectors::ensure_active_embedding_profile(&paths).unwrap();
    let collections = vectors::collection_names(&paths, &profile);
    assert_eq!(
        vectors::collection_point_count(&paths, &collections.text)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        vectors::collection_point_count(&paths, &collections.image)
            .await
            .unwrap(),
        2
    );
}

#[test]
fn smoke_transcript_lines_are_not_indexed_in_fts() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let conn = sqlite::open(&paths).unwrap();

    conn.execute(
        "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO items (id, source_id, content_type, status) VALUES ('item-1', 'source-1', 'video', 'ready')",
        [],
    )
    .unwrap();

    let transcript_chunks = vec![StorageTranscriptChunk {
        start: 0.0,
        end: 12.0,
        text: "retrieval searchable block".to_string(),
    }];
    let transcript_lines = vec![StorageTranscriptLine {
        start: 0.8,
        end: 2.1,
        text: "lineonly precise subtitle".to_string(),
    }];

    let summary = cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
        &paths,
        "item-1",
        &transcript_chunks,
        &transcript_lines,
        &[],
        &[],
    )
    .unwrap();

    assert_eq!(summary.transcript_chunks, 1);
    assert_eq!(match_count(&conn, "retrieval"), 1);
    assert_eq!(match_count(&conn, "lineonly"), 0);

    let line_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'transcript_line'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(line_count, 1);
}

fn match_count(conn: &rusqlite::Connection, term: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
        [term],
        |row| row.get(0),
    )
    .unwrap()
}

fn fake_vector(seed: usize) -> Vec<f32> {
    let mut vector = vec![0.0; vectors::VECTOR_DIMENSIONS as usize];
    vector[seed] = 1.0;
    vector
}
