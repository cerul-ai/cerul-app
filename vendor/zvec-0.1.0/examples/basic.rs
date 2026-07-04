//! Port of `examples/c/basic_example.c` from the zvec source tree.
//!
//! Builds a 3-dim HNSW collection, inserts two docs, flushes, runs a vector
//! query, and prints the results.

use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams, IndexType, MetricType,
    VectorQuery,
};

fn main() -> zvec::Result<()> {
    println!("=== zvec basic example ===");

    // Build the schema.
    let mut schema = CollectionSchema::new("test_collection")?;

    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;

    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    let mut id_field = FieldSchema::new("id", DataType::String, false, 0)?;
    id_field.set_index_params(&invert)?;
    schema.add_field(&id_field)?;

    let mut text_field = FieldSchema::new("text", DataType::String, true, 0)?;
    text_field.set_index_params(&invert)?;
    schema.add_field(&text_field)?;

    let mut emb_field = FieldSchema::new("embedding", DataType::VectorFp32, false, 3)?;
    emb_field.set_index_params(&hnsw)?;
    schema.add_field(&emb_field)?;

    // Create collection at a temp path so reruns don't clash.
    let tmp = std::env::temp_dir().join("zvec_rs_basic_example");
    let _ = std::fs::remove_dir_all(&tmp);
    let path = tmp.to_string_lossy().to_string();

    let collection = Collection::create_and_open(&path, &schema, None)?;
    println!("[ok] collection created at {path}");

    // Two documents.
    let mut d1 = Doc::new()?;
    d1.set_pk("doc1")?;
    d1.add_string("id", "doc1")?;
    d1.add_string("text", "First document")?;
    d1.add_vector_fp32("embedding", &[0.1, 0.2, 0.3])?;

    let mut d2 = Doc::new()?;
    d2.set_pk("doc2")?;
    d2.add_string("id", "doc2")?;
    d2.add_string("text", "Second document")?;
    d2.add_vector_fp32("embedding", &[0.4, 0.5, 0.6])?;

    let summary = collection.insert(&[&d1, &d2])?;
    println!(
        "[ok] inserted: success={}, error={}",
        summary.success, summary.error
    );

    collection.flush()?;
    println!("[ok] flushed");

    let stats = collection.stats()?;
    println!("[ok] stats: doc_count={}", stats.doc_count());

    // Vector query.
    let mut q = VectorQuery::new()?;
    q.set_field_name("embedding")?;
    q.set_query_vector_fp32(&[0.1, 0.2, 0.3])?;
    q.set_topk(10)?;
    q.set_include_vector(true)?;
    q.set_include_doc_id(true)?;

    let results = collection.query(&q)?;
    println!("[ok] query returned {} results", results.len());
    for (i, row) in results.iter().enumerate().take(5) {
        let pk = row.pk_copy().unwrap_or_else(|| "<null>".to_string());
        println!(
            "  result {}: pk={} doc_id={} score={:.4}",
            i + 1,
            pk,
            row.doc_id(),
            row.score()
        );
    }

    Ok(())
}
