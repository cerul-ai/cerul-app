# Cerul Local Vector Store Benchmark

Date: 2026-06-27

This benchmark compares local vector-store candidates for Cerul Desktop. The goal is not to reproduce a public ANN leaderboard. The goal is to answer which option fits a local, offline, subscription desktop app where SQLite remains the source of truth and the vector index is rebuildable.

## Scope

Tested:

- `zvec 0.5.1`
- `LanceDB 0.33.0`, exact search
- `LanceDB 0.33.0`, `IVF_HNSW_PQ` indexed search
- `sqlite-vec 0.1.9`
- `USearch 2.25.3`, default and high-recall modes
- `turbovec 0.8.0`

Not tested as local desktop candidates:

- Pinecone and turbopuffer, because they are cloud services and do not solve Cerul's offline local index problem.
- Weaviate local/embedded, because it keeps the service/sidecar shape that this migration is trying to avoid.
- Chroma local/server modes, because they keep a Python/service-oriented shape that does not align with the Rust/Electron packaged local core; `chromadb` was also left out of committed dependencies after dependency review flagged a critical advisory.

## Method

Environment:

- macOS arm64, Python 3.11.5
- Current repo: `/Users/jessytsui/cerul-ai/cerul-app`

Dataset:

- Synthetic normalized `float32` vectors.
- Main dimensionality: `2048`, matching Cerul's local embedding profile dimension.
- Queries are noisy variants of existing vectors, which approximates a query embedding landing near a relevant transcript/OCR retrieval unit.
- Ground truth is brute-force cosine top-k.

Measured:

- Build/write time.
- Query average, p50, p95 latency.
- Recall@10 against brute-force ground truth.
- Delete behavior.
- Reopen behavior.
- Concurrent read/write smoke behavior.
- On-disk size.

Raw artifacts:

- `.artifacts/vector-db-bench/10k-2048/results.json`
- `.artifacts/vector-db-bench/10k-2048-tuned/results.json`
- `.artifacts/vector-db-bench/30k-2048/results.json`
- `.artifacts/vector-db-bench/30k-2048-tuned/results.json`
- `.artifacts/vector-db-bench/50k-2048-core/results.json`
- `.artifacts/vector-db-bench/50k-2048-zvec-sqlite-fixed/results.json`
- `.artifacts/vector-db-bench/50k-2048-lancedb-indexed/results.json`

Reproduce:

```bash
.artifacts/vector-db-bench/venv/bin/python benchmarks/vector-db/bench_vector_dbs.py \
  --n 50000 \
  --dim 2048 \
  --queries 50 \
  --k 10 \
  --out-dir .artifacts/vector-db-bench/50k-2048-core \
  --backends zvec,lancedb,sqlite_vec,usearch_high_recall,turbovec
```

## 50k x 2048 Core Results

| Backend | Build s | Avg ms | P95 ms | Recall@10 | Disk MB | Delete | Reopen | Concurrent read/write |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- | --- |
| zvec | 4.02 | 12.96 | 13.38 | 1.000 | 404 | ok | ok | ok |
| sqlite-vec | 17.69 | 69.16 | 70.13 | 1.000 | 394 | ok | ok | ok |
| LanceDB exact | 0.44 | 84.68 | 90.48 | 1.000 | 391 | ok | ok | ok |
| USearch high-recall | 11.60 | 15.28 | 16.30 | 0.804 | 202 | ok | ok | ok |
| turbovec | 1.17 | 2.12 | 2.57 | 0.824 | 49 | ok | ok | ok |
| LanceDB IVF_HNSW_PQ | 17.28 | 6.22 | 12.06 | 0.204 | 403 | ok | ok | ok |

## Scaling Snapshot

| Backend | 10k avg ms / recall | 30k avg ms / recall | 50k avg ms / recall |
| --- | ---: | ---: | ---: |
| zvec | 2.43 / 1.000 | 7.41 / 1.000 | 12.96 / 1.000 |
| sqlite-vec | 15.48 / 1.000 | 43.23 / 1.000 | 69.16 / 1.000 |
| LanceDB exact | 15.09 / 1.000 | 37.53 / 1.000 | 84.68 / 1.000 |
| USearch high-recall | 5.07 / 0.990 | 11.73 / 0.925 | 15.28 / 0.804 |
| turbovec | 0.60 / 0.851 | 1.28 / 0.853 | 2.12 / 0.824 |

## Findings

### zvec

Best local default candidate from this benchmark.

Strengths:

- Fast enough at 50k x 2048: about 13 ms average query latency with perfect recall on this synthetic benchmark.
- In-process local library, so it avoids sidecar, port, readiness, proxy, and process lifecycle risks.
- Delete, post-close reopen, concurrent read, and concurrent write smoke tests passed.
- Disk footprint is roughly raw-vector-sized plus index overhead, not tiny but reasonable.

Risks:

- Single collection lock: while one `zvec` collection handle is open, a second handle cannot open the same collection, even read-only. Cerul should route vector access through one backend owner and avoid helper processes opening the same index directly.
- Newer ecosystem than LanceDB and sqlite-vec. We still need Rust integration, crash-recovery, packaging, and real Cerul data tests.

### sqlite-vec

Best conservative fallback, not best default.

Strengths:

- Very small dependency surface.
- Fits Cerul's existing SQLite ownership model.
- Exact recall.
- Delete, reopen, concurrent read/write smoke passed after using a thread-compatible connection.

Weaknesses:

- Query latency scales linearly. At 50k x 2048, average query latency was about 69 ms.
- Good fallback for small libraries and reliability-first mode, but not ideal as the primary vector index if user libraries grow.

### LanceDB

Good data-layer candidate, not the best local search default from this test.

Strengths:

- Very fast bulk table creation.
- Exact search is accurate.
- Stronger fit if Cerul later wants local multimodal tables, versioned datasets, or a cloud/data-lake continuity story.

Weaknesses:

- Exact query latency was slower than zvec and grows linearly.
- `IVF_HNSW_PQ` indexed mode was fast, but recall was poor under the tested 2048-dimensional workload.
- Tuning `nprobes/refine_factor` improved 10k recall, but at 10k, `nprobes=100/refine=200` still only reached about 0.857 recall at about 17.5 ms average, worse than zvec.

### USearch

Useful engine, not a complete DB.

Strengths:

- Lightweight.
- High-recall mode is competitive at 10k and 30k.
- Good low-level index engine if Cerul wants to own metadata, transaction, rebuild, and persistence semantics itself.

Weaknesses:

- Default recall was too low for product search.
- High-recall mode degraded from 0.99 at 10k to 0.804 at 50k in this benchmark.
- It is an index engine, not a complete local database. SQLite would still need to own all metadata and lifecycle state.

### turbovec

Interesting compression/performance experiment, not default.

Strengths:

- Very fast query latency.
- Very small disk footprint.
- Installed cleanly on macOS arm64.

Weaknesses:

- 4-bit quantized recall stayed around 0.82-0.85 in this benchmark.
- It is an index engine, not a full DB.
- Good for a future "compact index" experiment, not for default user-facing semantic search.

### Chroma

Not carried forward as a committed benchmark dependency.

Reasons:

- Python/local-development shape does not align well with Cerul's Rust/Electron local backend.
- Server mode brings back sidecar/service concerns.
- `chromadb 1.5.9` was flagged by dependency review for a critical advisory, so it should stay out of default local benchmark dependencies unless we explicitly revisit it with a patched version.

## Recommendation

Use `zvec` as the next serious local vector-store spike, behind a feature flag, with SQLite remaining the source of truth.

Recommended architecture:

- SQLite continues to own `items`, `retrieval_units`, FTS, jobs, providers, settings, index status, metadata, and rebuild state.
- zvec stores only rebuildable vector points: `point_id`, `retrieval_unit_id`, `item_id`, `embedding_profile_id`, `index_version`, `unit_kind`, optional timing fields, and the vector.
- Search continues to fuse SQLite FTS results with vector results.

Do not migrate local metadata ownership into the vector store. The vector index should remain disposable and rebuildable from SQLite.

Fallback plan:

- If zvec's Rust packaging, crash recovery, or lock behavior fails in the real app, use `sqlite-vec` as the reliability-first fallback for smaller local libraries.
- Keep `USearch` and `turbovec` as experimental engines only.
- Keep LanceDB in mind for future local multimodal dataset management, but not as the default vector-search replacement from this benchmark.

Next required test before implementation:

- Export a real Cerul retrieval benchmark from local `retrieval_units`: Chinese transcript, OCR, visual text, long videos, repeated reindex/delete cycles, and manually labeled queries with expected `item_id` plus time range.
- Re-run the same benchmark with real embeddings from the active local model.
