//! `HybridSearch` — run several queries against a [`Collection`] and
//! fuse their result lists into one ranked output.
//!
//! Hybrid retrieval typically combines multiple signals — a vector
//! similarity query, a vector query restricted by a `filter` (acting as
//! a "keyword" gate), or two vector queries against different fields.
//! `HybridSearch` does the boilerplate of running them all and feeding
//! the results to a [`crate::rerank::RrfReRanker`] (or
//! [`crate::rerank::WeightedReRanker`]) without forcing the caller to
//! manage intermediate `Vec<Hit>`s.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> zvec::Result<()> {
//! use zvec::{Collection, HybridSearch, VectorQuery};
//! use zvec::rerank::RrfReRanker;
//!
//! # let collection: Collection = unreachable!();
//! let mut text_q = VectorQuery::new()?;
//! text_q.set_field_name("title_embedding")?;
//! text_q.set_query_vector_fp32(&[0.1, 0.2, 0.3])?;
//! text_q.set_topk(50)?;
//!
//! let mut body_q = VectorQuery::new()?;
//! body_q.set_field_name("body_embedding")?;
//! body_q.set_query_vector_fp32(&[0.4, 0.5, 0.6])?;
//! body_q.set_topk(50)?;
//!
//! let hits = HybridSearch::new()
//!     .query(text_q)
//!     .query(body_q)
//!     .reranker(RrfReRanker::default())
//!     .top_k(10)
//!     .execute(&collection)?;
//!
//! for hit in hits {
//!     println!("{} score={:.4}", hit.pk, hit.score);
//! }
//! # Ok(())
//! # }
//! ```

use crate::collection::Collection;
use crate::error::Result;
use crate::query::VectorQuery;
use crate::rerank::{Hit, RrfReRanker, WeightedReRanker};

/// Builder that runs a set of [`VectorQuery`]s and fuses their results.
pub struct HybridSearch {
    queries: Vec<VectorQuery>,
    fuser: FuserKind,
    top_k: Option<usize>,
}

enum FuserKind {
    Rrf(RrfReRanker),
    Weighted(WeightedReRanker),
}

impl Default for HybridSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl HybridSearch {
    /// Empty builder. Defaults: RRF (k = 60), no top_k cap.
    pub fn new() -> Self {
        Self {
            queries: Vec::new(),
            fuser: FuserKind::Rrf(RrfReRanker::default()),
            top_k: None,
        }
    }

    /// Add a query to the bundle. Order doesn't affect RRF; for
    /// [`WeightedReRanker`] it must match the order of the configured
    /// weights.
    pub fn query(mut self, query: VectorQuery) -> Self {
        self.queries.push(query);
        self
    }

    /// Use Reciprocal Rank Fusion. Equivalent to `Self::new()`'s default.
    pub fn reranker(mut self, r: RrfReRanker) -> Self {
        self.fuser = FuserKind::Rrf(r);
        self
    }

    /// Use weighted linear fusion instead of RRF.
    pub fn weighted_reranker(mut self, r: WeightedReRanker) -> Self {
        self.fuser = FuserKind::Weighted(r);
        self
    }

    /// Cap the returned list to the top `n` fused hits.
    pub fn top_k(mut self, n: usize) -> Self {
        self.top_k = Some(n);
        self
    }

    /// Run every configured query against `collection`, fuse the results,
    /// and return them sorted best-first.
    pub fn execute(self, collection: &Collection) -> Result<Vec<Hit>> {
        let mut per_query: Vec<Vec<Hit>> = Vec::with_capacity(self.queries.len());
        for q in &self.queries {
            per_query.push(collection.query(q)?.to_hits());
        }
        let mut fused = match self.fuser {
            FuserKind::Rrf(r) => r.fuse(per_query),
            FuserKind::Weighted(r) => r.fuse(per_query),
        };
        if let Some(k) = self.top_k {
            fused.truncate(k);
        }
        Ok(fused)
    }
}

#[cfg(test)]
mod tests {
    // Unit tests for HybridSearch live in tests/integration.rs because
    // they need a real Collection — we test fusion logic in
    // crate::rerank::tests, and this module is just plumbing on top.
}
