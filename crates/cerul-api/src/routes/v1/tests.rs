use std::{fs, path::Path as FsPath};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::Response,
};
use cerul_storage::AppPaths;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::*;
use crate::router_with_paths;

fn seed_indexing_schema_version(paths: &AppPaths) {
    let conn = cerul_storage::sqlite::open(paths).unwrap();
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES
            (?1, ?2, strftime('%s','now')),
            (?3, ?4, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            crate::INDEXING_SCHEMA_VERSION_SETTING,
            Value::from(crate::INDEXING_SCHEMA_VERSION).to_string(),
            crate::VECTOR_INDEX_BACKEND_SETTING,
            Value::from(crate::ACTIVE_VECTOR_INDEX_BACKEND).to_string(),
        ),
    )
    .unwrap();
}

async fn response_json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn router_serves_v1_status_and_openapi() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    {
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, status, indexed_at, metadata
            )
            VALUES
                ('item-indexed', 'source-1', 'video', 'video-1', 'Indexed', 'indexed', 10, '{}'),
                ('item-processing', 'source-1', 'video', 'video-2', 'Processing', 'processing', NULL, '{}'),
                ('item-failed', 'source-1', 'video', 'video-3', 'Failed', 'failed', NULL, '{}')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO jobs (id, item_id, job_type, status, progress)
            VALUES ('job-queued', 'item-processing', 'index_video', 'queued', 0)
            "#,
            [],
        )
        .unwrap();
    }
    seed_indexing_schema_version(&paths);
    cerul_storage::set_item_search_index_status(&paths, "item-indexed", "indexed", None, 0, 0)
        .unwrap();
    let app = router_with_paths(paths);

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status_json = response_json(status).await;
    assert!(status_json["request_id"]
        .as_str()
        .unwrap()
        .starts_with("req-"));
    assert_eq!(status_json["status"], "ok");
    assert_eq!(status_json["execution"]["target"], "local");
    assert_eq!(status_json["execution"]["privacy"], "local_only");
    assert_eq!(status_json["library"]["total_items"], 3);
    assert_eq!(status_json["library"]["indexed_items"], 1);
    assert_eq!(status_json["library"]["processing_items"], 1);
    assert_eq!(status_json["library"]["failed_items"], 1);
    assert_eq!(status_json["indexing"]["queued_jobs"], 1);
    assert_eq!(status_json["account"]["signed_in"], false);
    assert_eq!(
        status_json["capabilities"],
        json!([
            "status",
            "openapi",
            "agent_tools",
            "search",
            "ask",
            "items",
            "chunks"
        ])
    );

    let openapi = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(openapi.status(), StatusCode::OK);
    let openapi_json = response_json(openapi).await;
    let paths = openapi_json["paths"].as_object().unwrap();
    assert!(paths.contains_key("/v1/status"));
    assert!(paths.contains_key("/v1/openapi.json"));
    assert!(paths.contains_key("/v1/agent/tools"));
    assert!(paths.contains_key("/v1/search"));
    assert!(paths.contains_key("/v1/ask"));
    assert!(paths.contains_key("/v1/items"));
    assert!(paths.contains_key("/v1/items/{id}"));
    assert!(paths.contains_key("/v1/items/{id}/chunks"));
    assert!(paths.contains_key("/v1/chunks/{id}/frame"));
    assert!(paths.contains_key("/v1/chunks/{id}/video-segment"));
    assert!(paths.contains_key("/v1/chunks/{id}/video-clip"));
    assert!(!paths.contains_key("/internal/health"));
    assert!(!paths.contains_key("/health"));
}

#[tokio::test]
async fn v1_status_ignores_legacy_chunks_fts_readiness() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    {
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, status, indexed_at, metadata
            )
            VALUES ('item-legacy', 'source-1', 'video', 'video-legacy', 'Legacy Indexed', 'indexed', 10, '{}')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES (
                'item-legacy:transcript:000000',
                'item-legacy',
                'transcript',
                1.0,
                2.0,
                'legacy chunks fts text should not mark v1 status ready',
                '{}'
            )
            "#,
            [],
        )
        .unwrap();
    }
    seed_indexing_schema_version(&paths);

    let app = router_with_paths(paths);
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status_json = response_json(response).await;
    assert_eq!(status_json["search"]["ready"], false);
    assert_eq!(status_json["search"]["retrieval_mode"], "empty");
    assert_eq!(status_json["search"]["text_ready"], false);
    assert_eq!(status_json["search"]["vector_ready"], false);
    assert_eq!(status_json["library"]["chunk_count"], 1);
}

#[tokio::test]
async fn v1_status_counts_pending_rebuilt_retrieval_units_as_text_ready() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    seed_status_retrieval_unit(
        &paths,
        "item-pending",
        "pending rebuilt status phrase",
        "pending",
    );
    seed_indexing_schema_version(&paths);

    let app = router_with_paths(paths);
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status_json = response_json(response).await;

    assert_eq!(status_json["search"]["ready"], true);
    assert_eq!(status_json["search"]["retrieval_mode"], "text");
    assert_eq!(status_json["search"]["text_ready"], true);
}

#[tokio::test]
async fn v1_status_ignores_failed_retrieval_units_fts_readiness() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    seed_status_retrieval_unit(
        &paths,
        "item-failed",
        "failed raw fts status phrase",
        "failed",
    );
    seed_indexing_schema_version(&paths);

    let app = router_with_paths(paths);
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status_json = response_json(response).await;

    assert_eq!(status_json["search"]["ready"], false);
    assert_eq!(status_json["search"]["retrieval_mode"], "empty");
    assert_eq!(status_json["search"]["text_ready"], false);
}

fn seed_status_retrieval_unit(
    paths: &AppPaths,
    item_id: &str,
    text: &str,
    search_index_status: &str,
) {
    {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (id, source_id, content_type, external_id, title, status, indexed_at, metadata)
            VALUES (?1, 'source-1', 'video', ?2, ?3, 'indexed', 10, '{}')
            "#,
            (item_id, item_id, item_id),
        )
        .unwrap();
    }
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
    let units = vec![cerul_storage::StorageRetrievalUnit {
        id: format!(
            "{item_id}:unit:v{}:000000",
            cerul_storage::SEARCH_INDEX_VERSION
        ),
        item_id: item_id.to_string(),
        unit_index: 0,
        unit_kind: "transcript".to_string(),
        start_sec: Some(1.0),
        end_sec: Some(2.0),
        content_text: text.to_string(),
        transcript_text: Some(text.to_string()),
        ocr_text: None,
        visual_text: None,
        summary_text: None,
        representative_chunk_id: None,
        representative_frame_path: None,
        embedding_profile_id: profile.id,
        index_version: cerul_storage::SEARCH_INDEX_VERSION,
        metadata: Default::default(),
    }];
    cerul_storage::replace_item_retrieval_units(paths, item_id, &units).unwrap();
    cerul_storage::set_item_search_index_status(
        paths,
        item_id,
        search_index_status,
        None,
        units.len(),
        0,
    )
    .unwrap();
}

fn seed_v1_agent_search_fixture(paths: &AppPaths, raw_path: &FsPath) {
    fs::write(raw_path, b"not a real video").unwrap();
    let raw_path_string = raw_path.to_string_lossy().to_string();
    let frame_path = raw_path.with_file_name("frame.jpg");
    fs::write(&frame_path, b"not a real frame").unwrap();
    let frame_path_string = frame_path.to_string_lossy().to_string();
    {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, duration_sec,
                raw_path, indexed_at, status, metadata
            )
            VALUES (
                'item-1', 'source-1', 'video', 'video-1', 'Scaling Talk', 120.5,
                ?1, 10, 'indexed', '{}'
            )
            "#,
            [raw_path_string.as_str()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES (
                'item-1:transcript:000000',
                'item-1',
                'transcript',
                12.3,
                18.0,
                'The talk says scaling laws keep holding across larger training runs.',
                '{}'
            )
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, frame_path, metadata)
            VALUES (
                'item-1:keyframe:000012',
                'item-1',
                'keyframe',
                12.0,
                12.0,
                ?1,
                '{}'
            )
            "#,
            [frame_path_string.as_str()],
        )
        .unwrap();
    }
    seed_indexing_schema_version(paths);
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
    let units = vec![cerul_storage::StorageRetrievalUnit {
        id: "item-1:unit:v2:000000".to_string(),
        item_id: "item-1".to_string(),
        unit_index: 0,
        unit_kind: "transcript".to_string(),
        start_sec: Some(12.3),
        end_sec: Some(18.0),
        content_text: "The talk says scaling laws keep holding across larger training runs."
            .to_string(),
        transcript_text: Some("scaling laws keep holding".to_string()),
        ocr_text: None,
        visual_text: None,
        summary_text: None,
        representative_chunk_id: Some("item-1:transcript:000000".to_string()),
        representative_frame_path: None,
        embedding_profile_id: profile.id,
        index_version: cerul_storage::SEARCH_INDEX_VERSION,
        metadata: Default::default(),
    }];
    cerul_storage::replace_item_retrieval_units(paths, "item-1", &units).unwrap();
    cerul_storage::set_item_search_index_status(paths, "item-1", "indexed", None, units.len(), 0)
        .unwrap();
}

fn seed_v1_document_search_fixture(paths: &AppPaths, raw_path: &FsPath) {
    fs::write(raw_path, b"document fixture").unwrap();
    let raw_path_string = raw_path.to_string_lossy().to_string();
    let source_config = json!({
        "path": raw_path.parent().unwrap_or_else(|| FsPath::new("."))
    })
    .to_string();
    {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-doc', 'folder_document', ?1, 'active')",
            [source_config.as_str()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title,
                raw_path, indexed_at, status, metadata
            )
            VALUES (
                'item-doc', 'source-doc', 'document', 'brief-doc', 'Launch Brief',
                ?1, 10, 'indexed', '{}'
            )
            "#,
            [raw_path_string.as_str()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, text, metadata)
            VALUES (
                'item-doc:document:000000',
                'item-doc',
                'document',
                'The document budget section mentions alpha launch positioning.',
                '{"page":2,"section":"Budget"}'
            )
            "#,
            [],
        )
        .unwrap();
    }
    seed_indexing_schema_version(paths);
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
    let units = vec![cerul_storage::StorageRetrievalUnit {
        id: "item-doc:unit:v2:000000".to_string(),
        item_id: "item-doc".to_string(),
        unit_index: 0,
        unit_kind: "document".to_string(),
        start_sec: None,
        end_sec: None,
        content_text: "Document: The document budget section mentions alpha launch positioning."
            .to_string(),
        transcript_text: Some(
            "The document budget section mentions alpha launch positioning.".to_string(),
        ),
        ocr_text: None,
        visual_text: None,
        summary_text: Some("Budget".to_string()),
        representative_chunk_id: Some("item-doc:document:000000".to_string()),
        representative_frame_path: None,
        embedding_profile_id: profile.id,
        index_version: cerul_storage::SEARCH_INDEX_VERSION,
        metadata: json!({ "page": 2, "section": "Budget" }),
    }];
    cerul_storage::replace_item_retrieval_units(paths, "item-doc", &units).unwrap();
    cerul_storage::set_item_search_index_status(paths, "item-doc", "indexed", None, units.len(), 0)
        .unwrap();
}

fn contract_shape(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(_) => Value::from("boolean"),
        Value::Number(_) => Value::from("number"),
        Value::String(_) => Value::from("string"),
        Value::Array(values) => Value::Array(values.iter().map(contract_shape).collect::<Vec<_>>()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), contract_shape(value)))
                .collect(),
        ),
    }
}

fn assert_contract_shape(name: &str, actual: &Value, expected: Value) {
    assert_eq!(
        contract_shape(actual),
        expected,
        "{name} contract shape changed"
    );
}

#[tokio::test]
async fn v1_golden_contract_shapes_cover_agent_endpoints() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    {
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('inference_mode', '"remote"', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();
    }
    let app = router_with_paths(paths);

    let openapi = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(openapi.status(), StatusCode::OK);
    let openapi = response_json(openapi).await;
    assert_eq!(openapi["openapi"], "3.1.0");
    assert_eq!(openapi["info"]["title"], "Cerul Agent API");
    assert_eq!(
        openapi["paths"],
        json!({
            "/v1/status": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/openapi.json": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/agent/tools": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/search": {"post": {"responses": {"200": {"description": "OK"}}}},
            "/v1/ask": {"post": {"responses": {"200": {"description": "OK"}}}},
            "/v1/items": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/items/{id}": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/items/{id}/chunks": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/chunks/{id}/frame": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/chunks/{id}/video-segment": {"get": {"responses": {"200": {"description": "OK"}}}},
            "/v1/chunks/{id}/video-clip": {"get": {"responses": {"200": {"description": "OK"}}}}
        })
    );

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status = response_json(status).await;
    assert_contract_shape(
        "v1 status",
        &status,
        json!({
            "request_id": "string",
            "status": "string",
            "version": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "library": {
                "total_items": "number",
                "indexed_items": "number",
                "processing_items": "number",
                "failed_items": "number",
                "chunk_count": "number"
            },
            "search": {
                "ready": "boolean",
                "retrieval_mode": "string",
                "text_ready": "boolean",
                "vector_ready": "boolean"
            },
            "indexing": {"paused": "boolean", "active_jobs": "number", "queued_jobs": "number"},
            "account": {"signed_in": "boolean", "plan": null, "credits_remaining": null},
            "capabilities": ["string", "string", "string", "string", "string", "string", "string"]
        }),
    );

    let agent_tools = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/agent/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(agent_tools.status(), StatusCode::OK);
    let agent_tools = response_json(agent_tools).await;
    assert_contract_shape(
        "v1 agent tools",
        &agent_tools,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "runtime": {
                "tool_host": "string",
                "renderer_access": "string",
                "arbitrary_shell": "boolean",
                "arbitrary_file_write": "boolean",
                "write_actions_require_confirmation": "boolean"
            },
            "tools": [{
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {
                        "max_results": {
                            "maximum": "number",
                            "minimum": "number",
                            "type": "string"
                        },
                        "query": {"minLength": "number", "type": "string"},
                        "target": {"enum": ["string"], "type": "string"}
                    },
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }, {
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {"id": {"minLength": "number", "type": "string"}},
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }, {
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {
                        "cursor": {"type": "string"},
                        "from_sec": {"minimum": "number", "type": "string"},
                        "id": {"minLength": "number", "type": "string"},
                        "limit": {"maximum": "number", "minimum": "number", "type": "string"},
                        "to_sec": {"minimum": "number", "type": "string"},
                        "type": {"type": "string"}
                    },
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }, {
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {"id": {"minLength": "number", "type": "string"}},
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }, {
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {"id": {"minLength": "number", "type": "string"}},
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }, {
                "name": "string",
                "description": "string",
                "method": "string",
                "path": "string",
                "stage": "string",
                "input_schema": {
                    "additionalProperties": "boolean",
                    "properties": {
                        "locale": {"type": "string"},
                        "max_results": {
                            "maximum": "number",
                            "minimum": "number",
                            "type": "string"
                        },
                        "mode": {"enum": ["string", "string"], "type": "string"},
                        "question": {"minLength": "number", "type": "string"},
                        "target": {"enum": ["string"], "type": "string"}
                    },
                    "required": ["string"],
                    "type": "string"
                },
                "output_contract": "string",
                "safety": {
                    "read_only": "boolean",
                    "billable": "boolean",
                    "requires_confirmation": "boolean",
                    "arbitrary_shell": "boolean",
                    "arbitrary_file_write": "boolean"
                },
                "evidence": {
                    "returns_evidence_locators": "boolean",
                    "opens_in_cerul": "boolean"
                }
            }]
        }),
    );
    assert_eq!(agent_tools["runtime"]["arbitrary_shell"], false);
    assert_eq!(agent_tools["runtime"]["arbitrary_file_write"], false);
    assert_eq!(
        agent_tools["execution"]["privacy"],
        "local_library_remote_query"
    );
    assert_eq!(
        agent_tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "search_library",
            "get_item",
            "get_chunks",
            "get_frame",
            "get_segment",
            "ask"
        ]
    );
    let tools = agent_tools["tools"].as_array().unwrap();
    let get_frame_tool = tools
        .iter()
        .find(|tool| tool["name"] == "get_frame")
        .unwrap();
    let get_segment_tool = tools
        .iter()
        .find(|tool| tool["name"] == "get_segment")
        .unwrap();
    assert_eq!(
        get_segment_tool["path"],
        "/v1/chunks/{id}/video-clip?before_sec=3&after_sec=5"
    );
    assert_eq!(
        get_frame_tool["evidence"]["returns_evidence_locators"],
        false
    );
    assert_eq!(get_frame_tool["evidence"]["opens_in_cerul"], false);
    assert_eq!(
        get_segment_tool["evidence"]["returns_evidence_locators"],
        false
    );
    assert_eq!(get_segment_tool["evidence"]["opens_in_cerul"], false);
    for tool in tools {
        let expected_read_only = tool["name"] != "get_segment";
        assert_eq!(tool["safety"]["read_only"], expected_read_only);
        assert_eq!(tool["safety"]["billable"], false);
        assert_eq!(tool["safety"]["requires_confirmation"], false);
        assert_eq!(tool["safety"]["arbitrary_shell"], false);
        assert_eq!(tool["safety"]["arbitrary_file_write"], false);
    }

    let search = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25101")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "scaling laws", "max_results": 1}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let search = response_json(search).await;
    assert_contract_shape(
        "v1 search",
        &search,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "results": [{
                "id": "string",
                "type": "string",
                "source": "string",
                "item": {
                    "id": "string",
                    "title": "string",
                    "content_type": "string",
                    "source_type": "string",
                    "duration_sec": "number"
                },
                "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                "text": {"snippet": "string", "quote": "string"},
                "evidence": {
                    "id": "string",
                    "kind": "string",
                    "clip": {"type": "string", "url": "string"},
                    "preview": {"type": "string", "url": "string"},
                    "open_in_cerul": "string"
                },
                "score": {"match": "number", "exact_match": "boolean", "similarity": null}
            }],
            "diagnostics": {
                "retrieval_mode": "string",
                "fallback_reason": "string",
                "vector_hits": "number",
                "text_hits": "number",
                "result_count": "number"
            },
            "usage": {
                "billable": "boolean",
                "metered_events": [
                    {"capability": "string", "quantity": "number", "credits": "number"},
                    {"capability": "string", "quantity": "number", "credits": "number"}
                ],
                "credits_used": "number"
            }
        }),
    );

    let ask = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::HOST, "127.0.0.1:25102")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"question": "scaling laws", "max_results": 1}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ask.status(), StatusCode::OK);
    let ask = response_json(ask).await;
    assert_contract_shape(
        "v1 ask",
        &ask,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "mode": "string",
            "answer": "string",
            "citations": [{
                "id": "string",
                "type": "string",
                "source": "string",
                "item": {
                    "id": "string",
                    "title": "string",
                    "content_type": "string",
                    "source_type": "string",
                    "duration_sec": "number"
                },
                "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                "text": {"snippet": "string", "quote": "string"},
                "evidence": {
                    "id": "string",
                    "kind": "string",
                    "clip": {"type": "string", "url": "string"},
                    "preview": {"type": "string", "url": "string"},
                    "open_in_cerul": "string"
                },
                "score": {"match": "number", "exact_match": "boolean", "similarity": null}
            }],
            "warnings": [],
            "usage": {
                "billable": "boolean",
                "metered_events": [
                    {"capability": "string", "quantity": "number", "credits": "number"},
                    {"capability": "string", "quantity": "number", "credits": "number"}
                ],
                "credits_used": "number"
            }
        }),
    );

    let items = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items?status=indexed&limit=1")
                .header(header::HOST, "127.0.0.1:25103")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items.status(), StatusCode::OK);
    let items = response_json(items).await;
    assert_contract_shape(
        "v1 items",
        &items,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "items": [{
                "id": "string",
                "title": "string",
                "content_type": "string",
                "source_type": "string",
                "source_url": null,
                "status": "string",
                "duration_sec": "number",
                "indexed_at": "number",
                "chunk_count": "number",
                "thumbnail": {"type": "string", "url": "string"},
                "open_in_cerul": "string"
            }],
            "page": {"limit": "number", "next_cursor": null}
        }),
    );

    let item = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1")
                .header(header::HOST, "127.0.0.1:25104")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(item.status(), StatusCode::OK);
    let item = response_json(item).await;
    assert_contract_shape(
        "v1 item",
        &item,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "item": {
                "id": "string",
                "title": "string",
                "content_type": "string",
                "source_type": "string",
                "source_url": null,
                "status": "string",
                "duration_sec": "number",
                "indexed_at": "number",
                "chunk_count": "number",
                "thumbnail": {"type": "string", "url": "string"},
                "open_in_cerul": "string"
            }
        }),
    );

    let chunks = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1/chunks?type=transcript&limit=1")
                .header(header::HOST, "127.0.0.1:25105")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(chunks.status(), StatusCode::OK);
    let chunks = response_json(chunks).await;
    assert_contract_shape(
        "v1 chunks",
        &chunks,
        json!({
            "request_id": "string",
            "execution": {"target": "string", "account_id": null, "privacy": "string"},
            "item": {
                "id": "string",
                "title": "string",
                "content_type": "string",
                "source_type": "string",
                "source_url": null,
                "status": "string",
                "duration_sec": "number",
                "indexed_at": "number",
                "chunk_count": "number",
                "thumbnail": {"type": "string", "url": "string"},
                "open_in_cerul": "string"
            },
            "chunks": [{
                "id": "string",
                "type": "string",
                "source": "string",
                "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                "text": {"content": "string", "snippet": "string"},
                "evidence": {
                    "id": "string",
                    "kind": "string",
                    "clip": {"type": "string", "url": "string"},
                    "preview": null,
                    "open_in_cerul": "string"
                }
            }],
            "page": {"limit": "number", "next_cursor": null}
        }),
    );

    // This golden fixture uses placeholder video bytes, so segment/clip binary
    // behavior stays covered by `v1_chunk_binary_routes_resolve_agent_evidence_urls`;
    // here the media contract locks the deterministic frame endpoint and the
    // OpenAPI paths for all three evidence media routes.
    let frame = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/chunks/item-1:keyframe:000012/frame")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(frame.status(), StatusCode::OK);
    assert_eq!(
        frame.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    let bytes = to_bytes(frame.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"not a real frame");
}

fn seed_v1_untimed_summary_fixture(paths: &AppPaths, raw_path: &FsPath) {
    fs::write(raw_path, b"not a real video").unwrap();
    let raw_path_string = raw_path.to_string_lossy().to_string();
    {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, duration_sec,
                raw_path, indexed_at, status, metadata
            )
            VALUES (
                'item-1', 'source-1', 'video', 'video-1', 'Summary Talk', 120.5,
                ?1, 10, 'indexed', '{}'
            )
            "#,
            [raw_path_string.as_str()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, text, metadata)
            VALUES (
                'item-1:understanding:summary',
                'item-1',
                'understanding',
                'Untimed executive summary about launch planning.',
                '{}'
            )
            "#,
            [],
        )
        .unwrap();
    }
    seed_indexing_schema_version(paths);
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
    let units = vec![cerul_storage::StorageRetrievalUnit {
        id: "item-1:unit:v2:summary".to_string(),
        item_id: "item-1".to_string(),
        unit_index: 0,
        unit_kind: "understanding".to_string(),
        start_sec: None,
        end_sec: None,
        content_text: "Untimed executive summary about launch planning.".to_string(),
        transcript_text: None,
        ocr_text: None,
        visual_text: None,
        summary_text: Some("Untimed executive summary about launch planning.".to_string()),
        representative_chunk_id: Some("item-1:understanding:summary".to_string()),
        representative_frame_path: None,
        embedding_profile_id: profile.id,
        index_version: cerul_storage::SEARCH_INDEX_VERSION,
        metadata: Default::default(),
    }];
    cerul_storage::replace_item_retrieval_units(paths, "item-1", &units).unwrap();
    cerul_storage::set_item_search_index_status(paths, "item-1", "indexed", None, units.len(), 0)
        .unwrap();
}

#[tokio::test]
async fn v1_items_omit_thumbnail_when_frame_file_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let missing_frame = temp.path().join("missing-frame.jpg");
    {
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, duration_sec,
                indexed_at, status, metadata
            )
            VALUES (
                'item-1', 'source-1', 'video', 'video-1', 'Clip', 10,
                10, 'indexed', '{}'
            )
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, frame_path, metadata)
            VALUES ('item-1:keyframe:000000', 'item-1', 'keyframe', 0, ?1, '{}')
            "#,
            [missing_frame.to_string_lossy().as_ref()],
        )
        .unwrap();
    }
    let app = router_with_paths(paths);

    let items = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items")
                .header(header::HOST, "127.0.0.1:25001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items.status(), StatusCode::OK);
    let items = response_json(items).await;
    assert!(items["items"][0]["thumbnail"].is_null());

    let item = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1")
                .header(header::HOST, "127.0.0.1:25001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(item.status(), StatusCode::OK);
    let item = response_json(item).await;
    assert!(item["item"]["thumbnail"].is_null());
}

#[tokio::test]
async fn v1_search_returns_agent_friendly_results_with_evidence_urls() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25001")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "scaling laws", "max_results": 2}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
    assert_eq!(body["execution"]["target"], "local");
    assert_eq!(body["results"][0]["id"], "item-1:transcript:000000");
    assert_eq!(body["results"][0]["type"], "transcript");
    assert_eq!(body["results"][0]["source"], "local_library");
    assert_eq!(body["results"][0]["item"]["id"], "item-1");
    assert_eq!(body["results"][0]["item"]["title"], "Scaling Talk");
    assert_eq!(body["results"][0]["item"]["content_type"], "video");
    assert_eq!(body["results"][0]["item"]["source_type"], "local");
    assert_eq!(body["results"][0]["item"]["duration_sec"], 120.5);
    assert_eq!(body["results"][0]["time"]["start_sec"], 12.3);
    assert_eq!(body["results"][0]["time"]["end_sec"], 18.0);
    assert_eq!(body["results"][0]["time"]["timestamp"], "0:12");
    assert!(body["results"][0]["text"]["snippet"]
        .as_str()
        .unwrap()
        .contains("scaling laws"));
    assert_eq!(body["results"][0]["evidence"]["kind"], "video_clip");
    assert_eq!(
        body["results"][0]["evidence"]["clip"]["url"],
        "http://127.0.0.1:25001/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
    );
    assert_eq!(
        body["results"][0]["evidence"]["preview"]["url"],
        "http://127.0.0.1:25001/v1/chunks/item-1%3Akeyframe%3A000012/frame"
    );
    assert_eq!(
        body["results"][0]["evidence"]["open_in_cerul"],
        "cerul-app://item/item-1?playbackChunkId=item-1%3Atranscript%3A000000&t=12.3"
    );
    assert_eq!(body["results"][0]["score"]["exact_match"], true);
    assert_eq!(body["usage"]["billable"], false);
    assert_eq!(
        body["usage"]["metered_events"][0],
        json!({"capability": "local_search", "quantity": 1, "credits": 0})
    );
}

#[tokio::test]
async fn v1_search_returns_document_page_and_section_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("launch-brief.md");
    seed_v1_document_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25001")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "alpha launch", "max_results": 1}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;

    assert_eq!(body["results"][0]["id"], "item-doc:document:000000");
    assert_eq!(body["results"][0]["type"], "document");
    assert_eq!(body["results"][0]["item"]["content_type"], "document");
    assert_eq!(body["results"][0]["evidence"]["kind"], "document");
    assert_eq!(body["results"][0]["evidence"]["page"], 2);
    assert_eq!(body["results"][0]["evidence"]["section"], "Budget");
    assert_eq!(
        body["results"][0]["evidence"]["open_in_cerul"],
        "cerul-app://item/item-doc?playbackChunkId=item-doc%3Adocument%3A000000&page=2"
    );
    assert!(body["results"][0]["text"]["snippet"]
        .as_str()
        .unwrap()
        .contains("alpha launch"));
}

#[tokio::test]
async fn v1_search_uses_q_alias_when_query_is_blank() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "   ", "q": "scaling laws"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["results"][0]["id"], "item-1:transcript:000000");
}

#[tokio::test]
async fn v1_search_marks_remote_embedding_privacy_when_remote_query_mode_is_selected() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    {
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('inference_mode', '"remote"', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();
    }
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["execution"]["target"], "local");
    assert_eq!(body["execution"]["privacy"], "local_library_remote_query");
    assert_eq!(
        body["usage"]["metered_events"],
        json!([
            {"capability": "local_search", "quantity": 1, "credits": 0},
            {"capability": "remote_embedding_query", "quantity": 1, "credits": 0}
        ])
    );
}

#[tokio::test]
async fn v1_search_does_not_advertise_clip_when_source_file_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    fs::remove_file(&raw_path).unwrap();
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25005")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let evidence = &body["results"][0]["evidence"];
    assert_eq!(evidence["kind"], "frame");
    assert_eq!(evidence["clip"], Value::Null);
    assert_eq!(
        evidence["preview"]["url"],
        "http://127.0.0.1:25005/v1/chunks/item-1%3Akeyframe%3A000012/frame"
    );
}

#[tokio::test]
async fn v1_search_does_not_advertise_preview_when_frame_file_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    let frame_path = temp.path().join("frame.jpg");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    fs::remove_file(frame_path).unwrap();
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25006")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let evidence = &body["results"][0]["evidence"];
    assert_eq!(evidence["kind"], "video_clip");
    assert!(evidence["clip"]["url"]
        .as_str()
        .unwrap()
        .contains("/v1/chunks/item-1%3Atranscript%3A000000/video-clip"));
    assert_eq!(evidence["preview"], Value::Null);
}

#[tokio::test]
async fn v1_search_does_not_advertise_clip_for_untimed_summary_hit() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_untimed_summary_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::HOST, "127.0.0.1:25007")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "untimed executive summary"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let evidence = &body["results"][0]["evidence"];
    assert_eq!(body["results"][0]["id"], "item-1:understanding:summary");
    assert_eq!(body["results"][0]["time"]["start_sec"], Value::Null);
    assert_eq!(evidence["kind"], "chunk");
    assert_eq!(evidence["clip"], Value::Null);
    assert_eq!(evidence["preview"], Value::Null);
    assert_eq!(
        evidence["open_in_cerul"],
        "cerul-app://item/item-1?playbackChunkId=item-1%3Aunderstanding%3Asummary"
    );
}

#[tokio::test]
async fn v1_search_rejects_cloud_target_until_cloud_proxy_exists() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"query": "scaling laws", "target": "cloud"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("only local or auto target"));
}

#[tokio::test]
async fn v1_ask_returns_extractive_answer_with_evidence_citations() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::HOST, "127.0.0.1:25002")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "question": "scaling laws",
                        "max_results": 2,
                        "locale": "en-US"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
    assert_eq!(body["execution"]["target"], "local");
    assert_eq!(body["mode"], "extractive");
    assert!(body["answer"]
        .as_str()
        .unwrap()
        .contains("This answer is extractive"));
    assert_eq!(body["citations"][0]["id"], "item-1:transcript:000000");
    assert_eq!(body["citations"][0]["item"]["title"], "Scaling Talk");
    assert_eq!(
        body["citations"][0]["evidence"]["clip"]["url"],
        "http://127.0.0.1:25002/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
    );
    assert_eq!(body["warnings"], json!([]));
    assert_eq!(body["usage"]["billable"], false);
    assert_eq!(
        body["usage"]["metered_events"][0],
        json!({"capability": "local_ask_extractive", "quantity": 1, "credits": 0})
    );
}

#[tokio::test]
async fn v1_ask_uses_document_page_and_section_in_answer() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("brief.md");
    seed_v1_document_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::HOST, "127.0.0.1:25002")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "question": "budget section alpha launch",
                        "max_results": 1,
                        "locale": "en-US"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let answer = body["answer"].as_str().unwrap();
    assert!(answer.contains("On page 2, section \"Budget\""));
    assert!(answer.contains("the document says"));
    assert!(!answer.contains("Around 0:00"));
    assert_eq!(body["citations"][0]["evidence"]["kind"], "document");
}

#[tokio::test]
async fn v1_ask_defaults_to_english_without_locale() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"question": "scaling laws"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let answer = body["answer"].as_str().unwrap();
    assert!(answer.contains("This answer is extractive"));
    assert!(!answer.contains("本回答"));
}

#[tokio::test]
async fn v1_ask_uses_fallback_aliases_after_trimming_blanks() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "question": "",
                        "query": "   ",
                        "q": "scaling laws"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["citations"][0]["id"], "item-1:transcript:000000");
}

#[tokio::test]
async fn v1_ask_rejects_non_extractive_mode_until_rag_exists() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/ask")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"question": "scaling laws", "mode": "rag"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("only extractive mode"));
}

#[tokio::test]
async fn v1_items_returns_agent_friendly_item_records() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items?status=indexed&limit=1")
                .header(header::HOST, "127.0.0.1:25003")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
    assert_eq!(body["execution"]["target"], "local");
    assert_eq!(body["page"], json!({"limit": 1, "next_cursor": null}));
    let item = &body["items"][0];
    assert_eq!(item["id"], "item-1");
    assert_eq!(item["title"], "Scaling Talk");
    assert_eq!(item["content_type"], "video");
    assert_eq!(item["source_type"], "local");
    assert_eq!(item["status"], "indexed");
    assert_eq!(item["duration_sec"], 120.5);
    assert_eq!(item["indexed_at"], 10);
    assert_eq!(item["chunk_count"], 2);
    assert_eq!(item["source_url"], Value::Null);
    assert_eq!(
        item["thumbnail"]["url"],
        "http://127.0.0.1:25003/v1/chunks/item-1%3Akeyframe%3A000012/frame"
    );
    assert_eq!(item["open_in_cerul"], "cerul-app://item/item-1");
    assert!(item.get("raw_path").is_none());
    assert!(item.get("metadata").is_none());
}

#[tokio::test]
async fn v1_item_chunks_returns_agent_context_with_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1/chunks?type=transcript&limit=5")
                .header(header::HOST, "127.0.0.1:25004")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["execution"]["target"], "local");
    assert_eq!(body["item"]["id"], "item-1");
    assert_eq!(body["chunks"].as_array().unwrap().len(), 1);
    let chunk = &body["chunks"][0];
    assert_eq!(chunk["id"], "item-1:transcript:000000");
    assert_eq!(chunk["type"], "transcript");
    assert_eq!(chunk["source"], "local_library");
    assert_eq!(chunk["time"]["start_sec"], 12.3);
    assert_eq!(chunk["time"]["end_sec"], 18.0);
    assert_eq!(chunk["time"]["timestamp"], "0:12");
    assert_eq!(
        chunk["text"]["content"],
        "The talk says scaling laws keep holding across larger training runs."
    );
    assert_eq!(
        chunk["evidence"]["clip"]["url"],
        "http://127.0.0.1:25004/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
    );
    assert_eq!(chunk["evidence"]["preview"], Value::Null);
    assert_eq!(
        chunk["evidence"]["open_in_cerul"],
        "cerul-app://item/item-1?playbackChunkId=item-1%3Atranscript%3A000000&t=12.3"
    );
    assert_eq!(body["page"], json!({"limit": 5, "next_cursor": null}));
}

#[tokio::test]
async fn v1_item_chunks_do_not_advertise_clip_for_untimed_chunks() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_untimed_summary_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1/chunks?type=summary")
                .header(header::HOST, "127.0.0.1:25008")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let chunk = &body["chunks"][0];
    assert_eq!(chunk["id"], "item-1:understanding:summary");
    assert_eq!(chunk["time"]["start_sec"], Value::Null);
    assert_eq!(chunk["evidence"]["kind"], "chunk");
    assert_eq!(chunk["evidence"]["clip"], Value::Null);
    assert_eq!(chunk["evidence"]["preview"], Value::Null);
    assert_eq!(
        chunk["evidence"]["open_in_cerul"],
        "cerul-app://item/item-1?playbackChunkId=item-1%3Aunderstanding%3Asummary"
    );
}

#[tokio::test]
async fn v1_item_chunks_translates_public_visual_type_filter() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path()).unwrap();
    let raw_path = temp.path().join("video.mp4");
    seed_v1_agent_search_fixture(&paths, &raw_path);
    let app = router_with_paths(paths);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/items/item-1/chunks?type=visual&limit=5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["chunks"].as_array().unwrap().len(), 1);
    assert_eq!(body["chunks"][0]["id"], "item-1:keyframe:000012");
    assert_eq!(body["chunks"][0]["type"], "visual");
}

#[test]
fn v1_chunk_type_filter_values_cover_public_aliases_and_raw_types() {
    assert_eq!(
        v1_chunk_type_filter_values("transcript"),
        vec!["transcript".to_string(), "transcript_line".to_string()]
    );
    assert_eq!(
        v1_chunk_type_filter_values("visual"),
        vec![
            "keyframe".to_string(),
            "image".to_string(),
            "ocr".to_string()
        ]
    );
    assert_eq!(
        v1_chunk_type_filter_values("summary"),
        vec!["understanding".to_string()]
    );
    assert_eq!(
        v1_chunk_type_filter_values("keyframe"),
        vec!["keyframe".to_string()]
    );
}
