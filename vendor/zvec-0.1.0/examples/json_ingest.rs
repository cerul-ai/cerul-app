//! Cookbook: ingesting JSON-shaped documents straight into a zvec
//! collection via the `serde-json` feature.
//!
//! Demonstrates `Doc::from_json` resolving field types through the
//! collection schema.
//!
//! Run with:
//!   cargo run --example json_ingest --features "bundled serde-json"

#[cfg(not(feature = "serde-json"))]
fn main() {
    eprintln!("re-run with --features \"bundled serde-json\"");
}

#[cfg(feature = "serde-json")]
fn main() -> zvec::Result<()> {
    use zvec::{
        Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams, IndexType,
        MetricType,
    };

    let path = tmp("zvec_cookbook_json");

    let mut schema = CollectionSchema::new("articles")?;
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

    let collection = Collection::create_and_open(&path, &schema, None)?;

    // Ingestion data could come from an HTTP request body, a JSONL log,
    // a config file, etc. — any source that produces serde_json::Value.
    let payload = serde_json::json!([
        { "_pk": "a", "id": "a", "embedding": [1.0, 0.0, 0.0] },
        { "_pk": "b", "id": "b", "embedding": [0.0, 1.0, 0.0] },
        { "_pk": "c", "id": "c", "embedding": [0.0, 0.0, 1.0] },
    ]);

    for v in payload.as_array().expect("expected an array") {
        let d = Doc::from_json(v, &schema)?;
        collection.insert(&[&d])?;
    }
    collection.flush()?;

    println!(
        "ingested {} docs from JSON",
        collection.stats()?.doc_count()
    );
    Ok(())
}

fn tmp(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(name);
    let _ = std::fs::remove_dir_all(&p);
    p.to_string_lossy().to_string()
}
