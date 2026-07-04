//! End-to-end integration tests.
//!
//! These require a prebuilt `libzvec_c_api` — point `ZVEC_LIB_DIR` (or
//! `ZVEC_ROOT`) at it and make sure `LD_LIBRARY_PATH` covers the same dir
//! before running `cargo test`.

use std::path::PathBuf;

use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams, IndexType, MetricType,
    VectorQuery,
};

fn tmp_path(name: &str) -> String {
    let mut p: PathBuf = std::env::temp_dir();
    p.push(format!("zvec_rs_test_{}_{}", name, std::process::id(),));
    let _ = std::fs::remove_dir_all(&p);
    p.to_string_lossy().to_string()
}

fn basic_schema() -> zvec::Result<CollectionSchema> {
    let mut schema = CollectionSchema::new("it_collection")?;
    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    let mut id = FieldSchema::new("id", DataType::String, false, 0)?;
    id.set_index_params(&invert)?;
    schema.add_field(&id)?;

    let mut emb = FieldSchema::new("embedding", DataType::VectorFp32, false, 3)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;
    Ok(schema)
}

#[test]
fn version_is_reported() {
    let v = zvec::version();
    assert!(!v.is_empty(), "version string was empty");
    assert!(zvec::version_major() >= 0);
}

#[test]
fn roundtrip_insert_query() -> zvec::Result<()> {
    let path = tmp_path("roundtrip");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let mut d1 = Doc::new()?;
    d1.set_pk("a")?;
    d1.add_string("id", "a")?;
    d1.add_vector_fp32("embedding", &[1.0, 0.0, 0.0])?;

    let mut d2 = Doc::new()?;
    d2.set_pk("b")?;
    d2.add_string("id", "b")?;
    d2.add_vector_fp32("embedding", &[0.0, 1.0, 0.0])?;

    let summary = collection.insert(&[&d1, &d2])?;
    assert_eq!(summary.success, 2);
    assert_eq!(summary.error, 0);
    collection.flush()?;
    assert_eq!(collection.stats()?.doc_count(), 2);

    let mut q = VectorQuery::new()?;
    q.set_field_name("embedding")?;
    q.set_query_vector_fp32(&[1.0, 0.0, 0.0])?;
    q.set_topk(2)?;
    let results = collection.query(&q)?;
    assert_eq!(results.len(), 2);
    // Closest result should be "a" (identical vector).
    let top = results.get(0).expect("first result");
    assert_eq!(top.pk_copy().as_deref(), Some("a"));
    Ok(())
}

#[test]
fn close_preserves_on_disk_collection() -> zvec::Result<()> {
    let path = tmp_path("close_preserves");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let mut doc = Doc::new()?;
    doc.set_pk("a")?;
    doc.add_string("id", "a")?;
    doc.add_vector_fp32("embedding", &[1.0, 0.0, 0.0])?;
    let summary = collection.insert(&[&doc])?;
    assert_eq!(summary.success, 1);
    collection.flush()?;
    collection.close()?;

    let reopened = Collection::open(&path, None)?;
    assert_eq!(reopened.stats()?.doc_count(), 1);
    let docs = reopened.fetch(&["a"])?;
    assert!(docs.iter().any(|doc| doc.pk_copy().as_deref() == Some("a")));
    Ok(())
}

#[test]
fn fetch_by_pk() -> zvec::Result<()> {
    let path = tmp_path("fetch");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let mut d = Doc::new()?;
    d.set_pk("only")?;
    d.add_string("id", "only")?;
    d.add_vector_fp32("embedding", &[0.5, 0.5, 0.5])?;
    collection.insert(&[&d])?;
    collection.flush()?;

    let got = collection.fetch(&["only", "missing"])?;
    assert!(got.len() <= 2);
    let found: Vec<_> = got.iter().map(|r| r.pk_copy()).collect();
    assert!(found.iter().any(|pk| pk.as_deref() == Some("only")));
    Ok(())
}

#[test]
fn delete_then_query_empty() -> zvec::Result<()> {
    let path = tmp_path("delete");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let mut d = Doc::new()?;
    d.set_pk("x")?;
    d.add_string("id", "x")?;
    d.add_vector_fp32("embedding", &[0.1, 0.1, 0.1])?;
    collection.insert(&[&d])?;
    collection.flush()?;

    let summary = collection.delete(&["x"])?;
    assert_eq!(summary.success, 1);
    collection.flush()?;
    Ok(())
}

#[cfg(feature = "serde-json")]
#[test]
fn from_json_roundtrip() -> zvec::Result<()> {
    let path = tmp_path("from_json");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let value = serde_json::json!({
        "_pk": "doc1",
        "id": "doc1",
        "embedding": [0.1, 0.2, 0.3],
    });
    let d = Doc::from_json(&value, &schema)?;
    collection.insert(&[&d])?;
    collection.flush()?;

    let results = collection.fetch(&["doc1"])?;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).and_then(|r| r.pk_copy()).as_deref(),
        Some("doc1")
    );
    Ok(())
}

#[cfg(feature = "serde-json")]
#[test]
fn from_json_rejects_unknown_field() -> zvec::Result<()> {
    let schema = basic_schema()?;
    let value = serde_json::json!({ "_pk": "x", "id": "x", "nope": 1 });
    match Doc::from_json(&value, &schema) {
        Ok(_) => panic!("unknown field should error"),
        Err(err) => {
            assert_eq!(err.code, zvec::ErrorCode::InvalidArgument);
            let msg = err.message.unwrap_or_default();
            assert!(msg.contains("nope"), "unexpected message: {msg}");
        }
    }
    Ok(())
}

#[test]
fn builder_roundtrip() -> zvec::Result<()> {
    let schema = CollectionSchema::builder("builder_collection")
        .field(FieldSchema::string("id").invert_index(true, false))
        .field(
            FieldSchema::vector_fp32("embedding", 3)
                .hnsw(16, 200)
                .metric(MetricType::Cosine),
        )
        .build()?;
    let path = tmp_path("builder");
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let mut d = Doc::new()?;
    d.set_pk("x")?;
    d.add_string("id", "x")?;
    d.add_vector_fp32("embedding", &[0.1, 0.2, 0.3])?;
    collection.insert(&[&d])?;
    collection.flush()?;

    let q = VectorQuery::builder()
        .field("embedding")
        .vector_fp32(&[0.1, 0.2, 0.3])
        .topk(1)
        .include_vector(true)
        .build()?;
    let results = collection.query(&q)?;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).and_then(|r| r.pk_copy()).as_deref(),
        Some("x")
    );
    Ok(())
}

#[cfg(feature = "half")]
#[test]
fn fp16_roundtrip() -> zvec::Result<()> {
    use half::f16;

    let mut schema = CollectionSchema::new("fp16_collection")?;
    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::L2)?;
    hnsw.set_hnsw_params(16, 200)?;

    let mut id = FieldSchema::new("id", DataType::String, false, 0)?;
    id.set_index_params(&invert)?;
    schema.add_field(&id)?;
    let mut emb = FieldSchema::new("embedding", DataType::VectorFp16, false, 3)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;

    let path = tmp_path("fp16");
    let coll = Collection::create_and_open(&path, &schema, None)?;

    let v = [f16::from_f32(0.25), f16::from_f32(0.5), f16::from_f32(0.75)];
    let mut d = Doc::new()?;
    d.set_pk("a")?;
    d.add_string("id", "a")?;
    d.add_vector_fp16("embedding", &v)?;
    coll.insert(&[&d])?;
    coll.flush()?;

    let mut q = VectorQuery::new()?;
    q.set_field_name("embedding")?;
    q.set_query_vector_fp16(&v)?;
    q.set_topk(1)?;
    q.set_include_vector(true)?;
    let results = coll.query(&q)?;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).and_then(|r| r.pk_copy()).as_deref(),
        Some("a")
    );
    Ok(())
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_insert_query_roundtrip() {
    use zvec::AsyncCollection;

    let path = tmp_path("async_rt");
    let schema = basic_schema().expect("schema");
    let collection = AsyncCollection::create_and_open(path, schema, None)
        .await
        .expect("create");

    let mut d = Doc::new().expect("doc");
    d.set_pk("only").expect("pk");
    d.add_string("id", "only").expect("id");
    d.add_vector_fp32("embedding", &[1.0, 0.0, 0.0])
        .expect("vec");
    let summary = collection.insert(vec![d]).await.expect("insert");
    assert_eq!(summary.success, 1);
    collection.flush().await.expect("flush");
    assert_eq!(collection.stats().await.expect("stats").doc_count(), 1);

    let mut q = VectorQuery::new().expect("query");
    q.set_field_name("embedding").expect("field");
    q.set_query_vector_fp32(&[1.0, 0.0, 0.0]).expect("vector");
    q.set_topk(1).expect("topk");
    let results = collection.query(q).await.expect("query exec");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).and_then(|r| r.pk_copy()).as_deref(),
        Some("only")
    );
}

#[test]
fn insert_iter_batches_correctly() -> zvec::Result<()> {
    let path = tmp_path("insert_iter");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    // 7 docs in batches of 3 → 3 + 3 + 1.
    let docs = (0..7).map(|i| {
        let mut d = Doc::new().unwrap();
        let pk = format!("d{i}");
        d.set_pk(&pk).unwrap();
        d.add_string("id", &pk).unwrap();
        d.add_vector_fp32("embedding", &[i as f32, 0.0, 0.0])
            .unwrap();
        d
    });
    let summary = collection.insert_iter(docs, 3)?;
    assert_eq!(summary.success, 7);
    assert_eq!(summary.error, 0);
    collection.flush()?;
    assert_eq!(collection.stats()?.doc_count(), 7);

    // Upsert variant: 3 fresh docs, batch size > input length.
    let more = (7..10).map(|i| {
        let mut d = Doc::new().unwrap();
        let pk = format!("d{i}");
        d.set_pk(&pk).unwrap();
        d.add_string("id", &pk).unwrap();
        d.add_vector_fp32("embedding", &[i as f32, 0.0, 0.0])
            .unwrap();
        d
    });
    let summary = collection.upsert_iter(more, 100)?;
    assert_eq!(summary.success, 3);
    collection.flush()?;
    assert_eq!(collection.stats()?.doc_count(), 10);
    Ok(())
}

#[cfg(feature = "derive")]
#[test]
fn derive_into_doc_roundtrip() -> zvec::Result<()> {
    use zvec::IntoDoc;

    #[derive(IntoDoc)]
    #[allow(dead_code)]
    struct Article {
        #[zvec(pk)]
        id: String,
        title: String,
        #[zvec(rename = "text")]
        body: String,
        #[zvec(vector_fp32)]
        embedding: Vec<f32>,
        summary: Option<String>,
        #[zvec(skip)]
        _audit: u64,
    }

    // Schema needs a `text` field because `body` was renamed.
    let mut schema = CollectionSchema::new("articles")?;
    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    for name in ["id", "title", "text"] {
        let mut f = FieldSchema::new(name, DataType::String, true, 0)?;
        f.set_index_params(&invert)?;
        schema.add_field(&f)?;
    }
    let mut summary = FieldSchema::new("summary", DataType::String, true, 0)?;
    summary.set_index_params(&invert)?;
    schema.add_field(&summary)?;
    let mut emb = FieldSchema::new("embedding", DataType::VectorFp32, false, 3)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;

    let path = tmp_path("derive");
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let a = Article {
        id: "a".into(),
        title: "Hello".into(),
        body: "body text".into(),
        embedding: vec![0.1, 0.2, 0.3],
        summary: None,
        _audit: 42,
    };
    let doc = a.into_doc()?;
    collection.insert(&[&doc])?;
    collection.flush()?;

    // Fetch back by pk.
    let results = collection.fetch(&["a"])?;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).and_then(|r| r.pk_copy()).as_deref(),
        Some("a")
    );
    Ok(())
}

#[cfg(feature = "derive")]
#[test]
fn derive_into_and_from_doc_roundtrip() -> zvec::Result<()> {
    use zvec::{FromDoc, IntoDoc};

    #[derive(IntoDoc, FromDoc, Debug, Default, PartialEq)]
    #[allow(dead_code)]
    struct Article {
        #[zvec(pk)]
        id: String,
        title: String,
        #[zvec(rename = "text")]
        body: String,
        #[zvec(vector_fp32)]
        embedding: Vec<f32>,
        views: i64,
        summary: Option<String>,
        #[zvec(skip)]
        audit: u64,
    }

    let mut schema = CollectionSchema::new("articles")?;
    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    for name in ["id", "title", "text", "summary"] {
        let mut f = FieldSchema::new(name, DataType::String, true, 0)?;
        f.set_index_params(&invert)?;
        schema.add_field(&f)?;
    }
    let mut views = FieldSchema::new("views", DataType::Int64, false, 0)?;
    views.set_index_params(&invert)?;
    schema.add_field(&views)?;
    let mut emb = FieldSchema::new("embedding", DataType::VectorFp32, false, 3)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;

    let path = tmp_path("derive_round");
    let collection = Collection::create_and_open(&path, &schema, None)?;

    let article = Article {
        id: "a".into(),
        title: "Hello".into(),
        body: "body text".into(),
        embedding: vec![0.1, 0.2, 0.3],
        views: 42,
        summary: Some("short summary".into()),
        audit: 9999, // skipped on insert; defaulted on read.
    };

    let doc = article.into_doc()?;
    collection.insert(&[&doc])?;
    collection.flush()?;

    // Fetch back and decode.
    let results = collection.fetch(&["a"])?;
    assert_eq!(results.len(), 1);
    let row = results.get(0).expect("row");
    let got = Article::from_doc(row)?;

    assert_eq!(got.id, "a");
    assert_eq!(got.title, "Hello");
    assert_eq!(got.body, "body text");
    // zvec's cosine/HNSW pipeline can round-trip vectors with
    // sub-ULP differences depending on the target's SIMD code path
    // (arm64 macOS returns e.g. 0.099999994 where x86_64 Linux
    // returns an exact 0.1). Compare with a small tolerance rather
    // than `assert_eq!`.
    assert_eq!(got.embedding.len(), 3);
    for (actual, expected) in got.embedding.iter().zip([0.1_f32, 0.2, 0.3]) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "embedding component {actual} diverged from {expected} by more than 1e-5",
        );
    }
    assert_eq!(got.views, 42);
    assert_eq!(got.summary.as_deref(), Some("short summary"));
    assert_eq!(got.audit, 0, "skipped field should default");

    // A doc without the `summary` field should decode with `summary = None`.
    let mut bare = Doc::new()?;
    bare.set_pk("b")?;
    bare.add_string("id", "b")?;
    bare.add_string("title", "")?;
    bare.add_string("text", "")?;
    bare.add_vector_fp32("embedding", &[0.0, 0.0, 0.0])?;
    bare.add_int64("views", 0)?;
    collection.insert(&[&bare])?;
    collection.flush()?;
    let results = collection.fetch(&["b"])?;
    let got = Article::from_doc(results.get(0).unwrap())?;
    assert_eq!(got.summary, None);

    Ok(())
}

#[test]
fn schema_introspection() -> zvec::Result<()> {
    let schema = basic_schema()?;
    assert_eq!(schema.name().as_deref(), Some("it_collection"));
    assert!(schema.has_field("id"));
    assert!(schema.has_field("embedding"));
    assert!(!schema.has_field("nope"));
    let names = schema.all_field_names()?;
    assert!(names.contains(&"id".to_string()));
    assert!(names.contains(&"embedding".to_string()));
    Ok(())
}

#[test]
fn hybrid_search_fuses_two_queries() -> zvec::Result<()> {
    use zvec::HybridSearch;

    let path = tmp_path("hybrid");
    let schema = basic_schema()?;
    let collection = Collection::create_and_open(&path, &schema, None)?;

    for (pk, v) in [
        ("a", [1.0, 0.0, 0.0]),
        ("b", [0.0, 1.0, 0.0]),
        ("c", [0.0, 0.0, 1.0]),
    ] {
        let mut d = Doc::new()?;
        d.set_pk(pk)?;
        d.add_string("id", pk)?;
        d.add_vector_fp32("embedding", &v)?;
        collection.insert(&[&d])?;
    }
    collection.flush()?;

    let mut q1 = VectorQuery::new()?;
    q1.set_field_name("embedding")?;
    q1.set_query_vector_fp32(&[1.0, 0.0, 0.0])?;
    q1.set_topk(3)?;

    let mut q2 = VectorQuery::new()?;
    q2.set_field_name("embedding")?;
    q2.set_query_vector_fp32(&[0.0, 1.0, 0.0])?;
    q2.set_topk(3)?;

    let hits = HybridSearch::new()
        .query(q1)
        .query(q2)
        .top_k(2)
        .execute(&collection)?;

    // The top-2 RRF-fused hits should be `a` and `b` (each ranks #1 in
    // exactly one query); `c` should not appear in the top 2.
    assert_eq!(hits.len(), 2);
    let pks: Vec<_> = hits.iter().map(|h| h.pk.as_str()).collect();
    assert!(pks.contains(&"a"));
    assert!(pks.contains(&"b"));
    assert!(!pks.contains(&"c"));
    Ok(())
}
