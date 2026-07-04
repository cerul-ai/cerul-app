//! Cookbook: end-to-end semantic search.
//!
//! Indexes a tiny corpus of (id, text, vector) docs in a fresh
//! collection at `$TMPDIR/zvec_cookbook_semantic`, then runs a vector
//! query and prints the top-3 results.
//!
//! Run with:
//!   cargo run --example semantic_search --features bundled

use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams, IndexType, MetricType,
    VectorQuery,
};

fn main() -> zvec::Result<()> {
    let path = tmp("zvec_cookbook_semantic");

    // --- schema ---
    let mut schema = CollectionSchema::new("articles")?;

    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;

    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    let mut id = FieldSchema::new("id", DataType::String, false, 0)?;
    id.set_index_params(&invert)?;
    schema.add_field(&id)?;

    let mut text = FieldSchema::new("text", DataType::String, true, 0)?;
    text.set_index_params(&invert)?;
    schema.add_field(&text)?;

    let mut emb = FieldSchema::new("embedding", DataType::VectorFp32, false, 4)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;

    let collection = Collection::create_and_open(&path, &schema, None)?;

    // --- ingest ---
    let corpus = [
        ("a", "rust ownership and borrowing", [0.9, 0.1, 0.0, 0.0]),
        ("b", "vector databases for embeddings", [0.0, 0.9, 0.1, 0.0]),
        ("c", "hnsw graph nearest neighbour", [0.05, 0.85, 0.10, 0.0]),
        ("d", "weather forecast for tomorrow", [0.0, 0.0, 0.0, 1.0]),
    ];
    for (pk, body, v) in corpus {
        let mut d = Doc::new()?;
        d.set_pk(pk)?;
        d.add_string("id", pk)?;
        d.add_string("text", body)?;
        d.add_vector_fp32("embedding", &v)?;
        collection.insert(&[&d])?;
    }
    collection.flush()?;

    // --- search ---
    let mut q = VectorQuery::new()?;
    q.set_field_name("embedding")?;
    q.set_query_vector_fp32(&[0.0, 1.0, 0.0, 0.0])?;
    q.set_topk(3)?;
    q.set_include_doc_id(true)?;

    let results = collection.query(&q)?;
    println!("top {} results:", results.len());
    for (i, row) in results.iter().enumerate() {
        let pk = row.pk_copy().unwrap_or_default();
        println!("  {}. {pk} score={:.4}", i + 1, row.score());
    }
    Ok(())
}

fn tmp(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(name);
    let _ = std::fs::remove_dir_all(&p);
    p.to_string_lossy().to_string()
}
