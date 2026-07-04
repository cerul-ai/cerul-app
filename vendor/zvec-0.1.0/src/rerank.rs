//! Result re-ranking / fusion helpers.
//!
//! These utilities operate on the output of [`Collection::query`][query]
//! (or any other source of `(pk, score)` pairs). They are pure Rust —
//! no zvec state is touched — so they're cheap to plug into a hybrid
//! retrieval pipeline that combines, say, a vector-similarity query
//! with a keyword-filter query and needs a single ranked list.
//!
//! - [`RrfReRanker`] — *Reciprocal rank fusion*. Only the **ordering**
//!   of each input list matters; absolute scores are thrown away.
//! - [`WeightedReRanker`] — linear combination of per-list scores with
//!   optional normalisation, for cases where scores are meaningful and
//!   comparable (same metric) across lists.
//!
//! [query]: crate::Collection::query
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> zvec::Result<()> {
//! use zvec::{Collection, VectorQuery, rerank::{Hit, RrfReRanker}};
//!
//! # let collection: Collection = unreachable!();
//! # let q_a: VectorQuery = unreachable!();
//! # let q_b: VectorQuery = unreachable!();
//! let a: Vec<Hit> = collection.query(&q_a)?.to_hits();
//! let b: Vec<Hit> = collection.query(&q_b)?.to_hits();
//!
//! let fused = RrfReRanker::default().fuse([a, b]);
//! for Hit { pk, score } in fused.into_iter().take(10) {
//!     println!("{pk} rrf={score:.4}");
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;

/// A single hit from a result set — primary key + a score where higher
/// means more relevant. Produced by [`crate::DocSet::to_hits`] or
/// constructed directly from an external source.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub pk: String,
    pub score: f32,
}

impl Hit {
    pub fn new(pk: impl Into<String>, score: f32) -> Self {
        Self {
            pk: pk.into(),
            score,
        }
    }
}

/// Reciprocal-rank fusion. Combines multiple ranked result lists by
/// summing `1 / (k + rank)` across the lists for every document that
/// appears in at least one.
///
/// Original paper: Cormack, Clarke & Büttcher (2009), "Reciprocal Rank
/// Fusion outperforms Condorcet and individual Rank Learning Methods."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RrfReRanker {
    /// RRF smoothing constant. 60 is conventional.
    pub k: f32,
}

impl Default for RrfReRanker {
    fn default() -> Self {
        Self { k: 60.0 }
    }
}

impl RrfReRanker {
    /// Construct with a custom `k`. Panics if `k` is non-finite or
    /// non-positive.
    pub fn new(k: f32) -> Self {
        assert!(
            k.is_finite() && k > 0.0,
            "RRF k must be positive and finite"
        );
        Self { k }
    }

    /// Fuse `lists`, each assumed sorted best-first. Returns hits sorted
    /// by descending fused score. Duplicate primary keys inside a single
    /// list are scored at their best (lowest) rank.
    pub fn fuse<L, I>(&self, lists: L) -> Vec<Hit>
    where
        L: IntoIterator<Item = I>,
        I: IntoIterator<Item = Hit>,
    {
        let mut agg: HashMap<String, f32> = HashMap::new();
        for list in lists {
            let mut seen: HashMap<String, usize> = HashMap::new();
            for (rank, hit) in list.into_iter().enumerate() {
                let best = seen.entry(hit.pk.clone()).or_insert(rank);
                if rank < *best {
                    *best = rank;
                }
            }
            for (pk, rank) in seen {
                let contribution = 1.0 / (self.k + rank as f32 + 1.0);
                *agg.entry(pk).or_insert(0.0) += contribution;
            }
        }
        sort_desc(agg)
    }
}

/// Score normalisation applied per-list before weighted fusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalization {
    /// Use raw scores as-is.
    None,
    /// Rescale each list's scores to `[0, 1]` via
    /// `(s - min) / (max - min)`. Lists of one element collapse to 1.0.
    MinMax,
}

/// Weighted linear fusion. Each list's scores are optionally normalised,
/// then summed with the per-list weight. Documents missing from a list
/// contribute `0` from that list.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedReRanker {
    pub weights: Vec<f32>,
    pub normalization: Normalization,
}

impl WeightedReRanker {
    /// `weights.len()` must match the number of lists passed to `fuse`.
    pub fn new(weights: Vec<f32>) -> Self {
        Self {
            weights,
            normalization: Normalization::MinMax,
        }
    }

    pub fn with_normalization(mut self, n: Normalization) -> Self {
        self.normalization = n;
        self
    }

    /// Fuse `lists`. Panics if the number of lists doesn't match
    /// [`Self::weights`]. Returns hits sorted by descending fused score.
    pub fn fuse<L, I>(&self, lists: L) -> Vec<Hit>
    where
        L: IntoIterator<Item = I>,
        I: IntoIterator<Item = Hit>,
    {
        let mut agg: HashMap<String, f32> = HashMap::new();
        let mut list_idx = 0usize;
        for list in lists {
            let hits: Vec<Hit> = list.into_iter().collect();
            let weight = *self.weights.get(list_idx).unwrap_or_else(|| {
                panic!(
                    "WeightedReRanker: got more lists than weights (weights.len() = {})",
                    self.weights.len()
                )
            });
            let (lo, hi) = extrema(&hits);
            for h in hits {
                let normalised = match self.normalization {
                    Normalization::None => h.score,
                    Normalization::MinMax => {
                        if hi > lo {
                            (h.score - lo) / (hi - lo)
                        } else {
                            1.0
                        }
                    }
                };
                *agg.entry(h.pk).or_insert(0.0) += weight * normalised;
            }
            list_idx += 1;
        }
        if list_idx != self.weights.len() {
            panic!(
                "WeightedReRanker: got {} lists but {} weights",
                list_idx,
                self.weights.len()
            );
        }
        sort_desc(agg)
    }
}

fn extrema(hits: &[Hit]) -> (f32, f32) {
    if hits.is_empty() {
        return (0.0, 0.0);
    }
    let mut lo = hits[0].score;
    let mut hi = hits[0].score;
    for h in &hits[1..] {
        if h.score < lo {
            lo = h.score;
        }
        if h.score > hi {
            hi = h.score;
        }
    }
    (lo, hi)
}

fn sort_desc(agg: HashMap<String, f32>) -> Vec<Hit> {
    let mut out: Vec<Hit> = agg
        .into_iter()
        .map(|(pk, score)| Hit { pk, score })
        .collect();
    // Sort by score desc; break ties on pk for determinism.
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.pk.cmp(&b.pk))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(pairs: &[(&str, f32)]) -> Vec<Hit> {
        pairs.iter().map(|(pk, s)| Hit::new(*pk, *s)).collect()
    }

    #[test]
    fn rrf_prefers_consensus_hits() {
        let a = hits(&[("doc1", 0.9), ("doc2", 0.8), ("doc3", 0.7)]);
        let b = hits(&[("doc2", 0.95), ("doc1", 0.9), ("doc4", 0.8)]);
        let fused = RrfReRanker::default().fuse([a, b]);
        // doc1 and doc2 are in both lists — they should come first. Order
        // between them depends on their summed reciprocal ranks:
        //   doc1: 1/(60+1) + 1/(60+2) ≈ 0.03244
        //   doc2: 1/(60+2) + 1/(60+1) ≈ 0.03244  (same — ties break on pk)
        assert_eq!(&fused[0].pk, "doc1");
        assert_eq!(&fused[1].pk, "doc2");
        assert!(fused[0].score > fused[2].score);
        assert_eq!(fused.len(), 4);
    }

    #[test]
    fn rrf_dedup_within_list_uses_best_rank() {
        let a = hits(&[("doc1", 1.0), ("doc1", 0.5), ("doc2", 0.4)]);
        let fused = RrfReRanker::new(10.0).fuse([a]);
        assert_eq!(fused.len(), 2);
        assert_eq!(&fused[0].pk, "doc1");
        // 1 / (10 + 0 + 1) = 1/11
        let expected = 1.0_f32 / 11.0;
        assert!((fused[0].score - expected).abs() < 1e-6);
    }

    #[test]
    fn weighted_minmax_flattens_score_scales() {
        // list a has scores in 0..=10, list b in 0..=1 — without
        // normalisation list a dominates; with MinMax they weigh
        // fairly.
        let a = hits(&[("doc1", 10.0), ("doc2", 5.0), ("doc3", 0.0)]);
        let b = hits(&[("doc3", 1.0), ("doc2", 0.5), ("doc1", 0.0)]);
        let fused = WeightedReRanker::new(vec![1.0, 1.0]).fuse([a, b]);
        // MinMax-normalised and summed:
        //   doc1: 1.0 + 0.0 = 1.0
        //   doc2: 0.5 + 0.5 = 1.0 -> tie with doc1; pk order breaks tie
        //   doc3: 0.0 + 1.0 = 1.0 -> tie.
        assert_eq!(fused.len(), 3);
        assert!((fused[0].score - 1.0).abs() < 1e-6);
        assert!((fused[2].score - 1.0).abs() < 1e-6);
        let pks: Vec<_> = fused.iter().map(|h| h.pk.as_str()).collect();
        assert_eq!(pks, ["doc1", "doc2", "doc3"]);
    }

    #[test]
    fn weighted_respects_weights() {
        let a = hits(&[("doc1", 1.0), ("doc2", 0.0)]);
        let b = hits(&[("doc2", 1.0), ("doc1", 0.0)]);
        // Weight list b 10x heavier → doc2 wins.
        let fused = WeightedReRanker::new(vec![1.0, 10.0])
            .with_normalization(Normalization::None)
            .fuse([a, b]);
        assert_eq!(fused.len(), 2);
        assert_eq!(&fused[0].pk, "doc2");
        assert!(fused[0].score > fused[1].score);
    }

    #[test]
    #[should_panic(expected = "got 1 lists but 2 weights")]
    fn weighted_rejects_too_few_lists() {
        let a = hits(&[("doc1", 1.0)]);
        WeightedReRanker::new(vec![1.0, 1.0]).fuse([a]);
    }

    #[test]
    #[should_panic(expected = "got more lists than weights")]
    fn weighted_rejects_too_many_lists() {
        let a = hits(&[("doc1", 1.0)]);
        let b = hits(&[("doc2", 1.0)]);
        WeightedReRanker::new(vec![1.0]).fuse([a, b]);
    }
}
