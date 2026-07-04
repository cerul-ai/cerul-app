//! Cookbook: hybrid search across two embeddings (e.g. title vs. body)
//! fused with Reciprocal Rank Fusion.
//!
//! Indexes a tiny corpus where each doc has separate `title_emb` and
//! `body_emb` vectors, runs a query against each, and uses
//! `HybridSearch` to combine the rankings.
//!
//! Run with:
//!   cargo run --example hybrid_search --features bundled

use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, HybridSearch, IndexParams, IndexType,
    MetricType, VectorQuery,
};

fn main() -> zvec::Result<()> {
    let path = tmp("zvec_cookbook_hybrid");

    let mut schema = CollectionSchema::new("articles")?;
    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;

    let mut id = FieldSchema::new("id", DataType::String, false, 0)?;
    id.set_index_params(&invert)?;
    schema.add_field(&id)?;

    for field_name in ["title_emb", "body_emb"] {
        let mut f = FieldSchema::new(field_name, DataType::VectorFp32, false, 3)?;
        f.set_index_params(&hnsw)?;
        schema.add_field(&f)?;
    }

    let collection = Collection::create_and_open(&path, &schema, None)?;

    // pk, title_emb, body_emb
    let corpus = [
        ("a", [0.9, 0.1, 0.0], [0.1, 0.9, 0.0]),
        ("b", [0.85, 0.15, 0.0], [0.05, 0.95, 0.0]),
        ("c", [0.0, 0.5, 0.5], [0.5, 0.5, 0.0]),
        ("d", [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]),
    ];
    for (pk, t, b) in corpus {
        let mut d = Doc::new()?;
        d.set_pk(pk)?;
        d.add_string("id", pk)?;
        d.add_vector_fp32("title_emb", &t)?;
        d.add_vector_fp32("body_emb", &b)?;
        collection.insert(&[&d])?;
    }
    collection.flush()?;

    // Two queries — one per embedding field.
    let mut q_title = VectorQuery::new()?;
    q_title.set_field_name("title_emb")?;
    q_title.set_query_vector_fp32(&[0.9, 0.1, 0.0])?;
    q_title.set_topk(10)?;

    let mut q_body = VectorQuery::new()?;
    q_body.set_field_name("body_emb")?;
    q_body.set_query_vector_fp32(&[0.0, 1.0, 0.0])?;
    q_body.set_topk(10)?;

    let hits = HybridSearch::new()
        .query(q_title)
        .query(q_body)
        .top_k(3)
        .execute(&collection)?;

    println!("hybrid top {}:", hits.len());
    for (i, hit) in hits.iter().enumerate() {
        println!("  {}. {} rrf={:.4}", i + 1, hit.pk, hit.score);
    }
    Ok(())
}

fn tmp(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(name);
    let _ = std::fs::remove_dir_all(&p);
    p.to_string_lossy().to_string()
}
