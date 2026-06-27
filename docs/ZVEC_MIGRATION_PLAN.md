# zvec Migration Notes for Cerul Desktop

Last updated: 2026-06-27

## Goal

Evaluate whether Cerul Desktop should replace the local Qdrant sidecar with
`zvec` as the on-device vector index.

The goal is not to move the whole local library into a new database. Cerul's
local source of truth should remain SQLite:

- `items`
- `chunks`
- `retrieval_units`
- `retrieval_units_fts`
- jobs, settings, providers, usage, and indexing state

The vector store should remain a derived index that can be deleted and rebuilt
from SQLite without losing transcripts, metadata, settings, local models, or
user history.

## Current Local Search Boundary

Today the local stack is:

- SQLite stores metadata, chunks, retrieval units, jobs, providers, settings,
  and FTS tables.
- Qdrant stores text and image/keyframe vectors.
- Search combines SQLite lexical recall with vector recall, then merges/ranks
  results.
- If embedding/vector indexing fails, transcript-first search must still work.

This boundary should survive any vector-store migration. The migration should
change the vector index implementation, not the product data model.

## Why zvec Is Worth Testing

`zvec` is attractive for Cerul because it is an embedded/in-process vector
database rather than a separately launched sidecar. That directly targets the
failure class we have seen with Qdrant:

- no loopback port ownership problem;
- no background Qdrant process to start, stop, or race;
- no sidecar readiness timeout;
- fewer macOS packaging and launch lifecycle edges;
- easier mental model for "local semantic index is a rebuildable cache."

The upstream project currently advertises capabilities that line up with Cerul's
needs: Rust bindings, WAL, DiskANN, filters, full-text search, and hybrid
retrieval. Treat those as claims to verify in a Cerul-specific spike, not as
already proven production behavior for our workload.

Reference: https://github.com/alibaba/zvec

## Benchmark Update: 2026-06-27

We ran a local benchmark under `benchmarks/vector-db/` using synthetic normalized
`float32` vectors at Cerul's local embedding dimensionality (`2048`).

The benchmark is not a replacement for a real Cerul retrieval-quality test, but
it is enough to guide the next implementation spike.

50k vectors, 2048 dimensions, top-10 search:

| Backend | Build s | Avg ms | P95 ms | Recall@10 | Disk MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| zvec | 4.02 | 12.96 | 13.38 | 1.000 | 404 |
| sqlite-vec | 17.69 | 69.16 | 70.13 | 1.000 | 394 |
| LanceDB exact | 0.44 | 84.68 | 90.48 | 1.000 | 391 |
| USearch high-recall | 11.60 | 15.28 | 16.30 | 0.804 | 202 |
| turbovec | 1.17 | 2.12 | 2.57 | 0.824 | 49 |
| Qdrant sidecar | 37.59 | 17.55 | 21.74 | 0.830 | 1498 |
| LanceDB IVF_HNSW_PQ | 17.28 | 6.22 | 12.06 | 0.204 | 403 |

Full benchmark notes: `benchmarks/vector-db/RESULTS.md`.

Implications:

- zvec is the best next local default candidate from the first benchmark.
- sqlite-vec is the strongest conservative fallback, especially for smaller
  libraries or reliability-first mode.
- Qdrant remains a useful baseline, but its sidecar shape keeps the exact local
  desktop failure class we are trying to remove.
- LanceDB remains interesting for future local multimodal dataset management,
  but the tested indexed mode was not a good default vector-search replacement.
- The benchmark used the repo's current bundled Qdrant binary (`1.18.1`), while
  the user failure log mentioned `1.18.2`; strict Qdrant regression work should
  retest that exact binary.

## Implementation Update: 2026-06-27

The first production migration pass replaces the local Qdrant sidecar with
zvec directly in `cerul-storage`.

Implemented decisions:

- `AppPaths` now uses `indexes/zvec` for the active local vector index.
- The Rust storage layer owns a process-wide zvec collection handle cache so the
  app does not open multiple handles to the same collection path.
- Existing search, pipeline, and API call sites still use Cerul-level vector
  operations such as replace, upsert, count, search, and fetch vectors.
- Search diagnostics and desktop API types now use `vector_index_*` fields
  instead of `qdrant_*`.
- Electron no longer stages, launches, injects, or checks a vector-database
  sidecar binary.
- zvec cosine scores are treated as distances and converted to similarity in
  search ranking with `1 - distance`.
- zvec collection schema names are shortened to stable `cerul_<uuid>` names
  because long Cerul collection names fail zvec schema validation. The full
  Cerul collection name remains the disk directory and diagnostic identifier.

Current implementation path:

```text
~/Library/Application Support/Cerul/indexes/zvec/
  collections/
    <cerul-logical-collection-name>/
```

The logical collection name already includes data-dir namespace, search index
version, and embedding profile id. This preserves API/local profile isolation
while keeping the zvec internal schema name short.

Known follow-up:

- `delete_stale_item_unified_embeddings_for_profile` now guarantees the final
  per-item vector set by deleting the item and re-upserting `keep_records`.
  zvec's filter delete API does not return deleted point ids, so the returned
  stale count is currently conservative.
- There is no automatic Qdrant-to-zvec vector export in this pass. Existing
  local semantic indexes should be rebuilt from SQLite retrieval units and
  embeddings.
- A user-facing "Rebuild semantic index" repair flow is still needed.

## Non-Goals

Do not use the migration to:

- move authoritative library metadata out of SQLite;
- make zvec the owner of item/chunk/retrieval unit records;
- rewrite ranking, chunking, or embedding model selection at the same time;
- remove SQLite FTS fallback;
- unify local and cloud vector-store implementations just for symmetry.

## Required Invariants

The migrated system must preserve these invariants:

1. **SQLite remains authoritative.**
   If zvec is deleted, Cerul can rebuild vectors from `retrieval_units`.

2. **Embedding profile isolation remains explicit.**
   Vectors for different models, dimensions, distance metrics, or index versions
   must not mix.

3. **Transcript-first indexing remains durable.**
   ASR and text chunks must remain searchable even if vector writes fail.

4. **Vector failures are partial failures.**
   A broken vector index should degrade semantic/media retrieval, not make the
   item look globally unindexed.

5. **Rebuild is a first-class repair path.**
   Users should have a safe "Rebuild semantic index" operation that resets only
   the derived vector index and requeues embedding writes.

6. **Cloud and local implementations may differ.**
   The product contract is the retrieval schema and ranking behavior, not a
   shared database product.

## Proposed Local Architecture

Keep SQLite as the product database:

```text
SQLite
  items
  chunks
  retrieval_units
  retrieval_units_fts
  embedding_profiles
  jobs/settings/providers

zvec
  vector indexes for retrieval_units
    per embedding profile
    per search/model index version boundary
    separate text and image branches
  optional local filter fields needed for fast top-k
```

Store the minimum needed payload in zvec:

- `point_id`
- `retrieval_unit_id`
- `item_id`
- `embedding_profile_id`
- `search_index_version`
- `embedding_profile_index_version`
- `vector_branch` (`text` or `image`)
- `unit_kind`
- `start_sec`
- `end_sec`
- vector

Everything else should be joined back from SQLite after candidate retrieval.
That keeps zvec small and rebuildable.

## Metadata, Versioning, Multimodal Tables, and Analytics

When people say a local vector database can own "complex metadata, versioning,
multimodal tables, and analytics," they usually mean the vector database stores
and queries much more than vectors:

- **Complex metadata:** tags, authors, source names, ACLs, timestamps,
  MIME types, model metadata, nested JSON, and rich filters directly inside the
  vector DB.
- **Versioning:** multiple versions of the same asset, snapshot history,
  rollback, or time-travel style dataset reads.
- **Multimodal tables:** text vectors, image vectors, audio vectors, raw
  captions, thumbnails, and structured metadata in the same table system.
- **Analytics queries:** aggregations, scans, offline dataset exploration,
  embedding distribution checks, quality dashboards, and batch analysis.

Cerul Desktop does not currently need the local vector store to own most of
that. We already have SQLite for metadata and FTS, and the local app's hot path
is interactive retrieval, not offline analytics.

Cerul already has multiple embedding profiles, including API and local-model
profiles with different dimensions. That is a real requirement, but it is
profile isolation, not dataset-style versioning. The migration must keep API
and local vectors in separate profile/index boundaries, but it does not require
moving multimodal metadata ownership from SQLite into LanceDB or another table
engine.

What Cerul does need locally:

- point lookup and top-k vector search;
- delete/rewrite all vectors for one item;
- filter or route by `embedding_profile_id`, `search_index_version`,
  `embedding_profile_index_version`, `vector_branch`, and maybe `unit_kind`;
- rebuild from SQLite;
- stable crash recovery;
- low startup overhead;
- simple macOS packaging.

So zvec should be evaluated as a local index engine. If we later need local
dataset-style analytics or richer multimodal tables, LanceDB may be a better
fit than zvec. For now, keeping metadata in SQLite is simpler and safer.

## Local vs Cloud Vector Stores

Local and cloud vector stores do not need to match.

Cloud can keep using PostgreSQL + pgvector for official material because cloud
has different constraints:

- subscription and entitlement checks;
- API keys and rate limits;
- shared official catalog metadata;
- billing and auditability;
- managed backup/restore;
- horizontal scale later.

Local should optimize for:

- zero user-visible operations;
- no local service management;
- crash recovery;
- offline search;
- rebuildability from local SQLite.

Search quality is not determined by whether local and cloud use the same vector
database. It is mainly determined by:

- embedding model and dimensions;
- distance metric;
- chunking and retrieval-unit construction;
- metadata filters;
- candidate oversampling;
- hybrid fusion and reranking.

Different vector stores can return slightly different approximate nearest
neighbor orderings. Cerul should handle that by fetching enough candidates and
applying shared reranking/fusion logic.

## Migration Work Items

### 1. Introduce a VectorIndex abstraction

Create a storage-level interface that covers the current Qdrant surface:

- ensure/open index for an embedding profile and vector branch;
- replace all item vectors;
- upsert item vectors;
- delete stale item vectors;
- delete all item vectors;
- count points for diagnostics;
- search top-k by vector with filters;
- report runtime/index diagnostics.

This interface should expose Cerul concepts, not zvec/Qdrant concepts.

### 2. Keep zvec payload minimal

Avoid duplicating the entire retrieval unit. Store only identifiers and filter
fields. Join selected candidates back to SQLite.

This limits migration risk and makes repair simple.

### 3. Define local index layout

Original proposed path:

```text
~/Library/Application Support/Cerul/indexes/zvec/
  profiles/
    <profile-id>/
      model-<embedding-profile-index-version>/
        search-<retrieval-unit-search-index-version>/
          text/
          image/
```

The path should include profile, model-index, search-index, and vector-branch
boundaries, or zvec must provide equivalent collection/table separation.

The implemented first pass uses the existing Cerul logical collection names as
the directory boundary under `indexes/zvec/collections/`. Those names already
include data-dir namespace, search index version, and embedding profile id.

Avoid relying on only a scalar `embedding_profile_id` filter inside one shared
zvec collection. Profile separation is safer because API and local embeddings
have different dimensions (`3072` vs `2048`) and cannot be searched together.

Also account for zvec's observed lock behavior: while one collection handle is
open, a second handle cannot open the same collection path, even read-only.
Cerul should therefore make the Rust API/storage layer the single owner of zvec
handles and route all local vector access through that owner.

### 4. Add rebuild and repair flows

Required operations:

- detect zvec index missing/corrupt/unopenable;
- quarantine the broken index directory instead of deleting blindly;
- rebuild from `retrieval_units`;
- keep transcripts and FTS searchable during rebuild;
- surface rebuild progress in existing job/indexing UI.

### 5. Preserve fallback behavior

If zvec search fails:

- log diagnostics;
- mark vector search degraded;
- fall back to SQLite FTS;
- do not fail the whole search request unless both vector and FTS paths fail.

### 6. Migrate without forcing immediate re-embedding

If SQLite already has `retrieval_units` and embeddings are available only in
Qdrant, migration may require re-embedding unless we can read/export vectors
from Qdrant. Decide explicitly:

- **Clean rebuild:** simpler, but costs time/API/local model compute.
- **Qdrant export:** faster for users with large indexes, but more migration
  code and more failure cases.

For the first spike, prefer clean rebuild.

### 7. Transition decision

The initial recommendation was to keep Qdrant behind a feature flag during a
transition. The implementation pass instead removes the running Qdrant backend
from the desktop runtime because the user-visible failure is specifically tied
to sidecar readiness, ports, and lifecycle.

If we need rollback coverage, prefer adding an internal storage-backend boundary
or sqlite-vec fallback rather than reintroducing Qdrant process management into
the Electron app.

## Spike Checklist

Run the same dataset through Qdrant and zvec.

Completed first-pass benchmark:

- synthetic 10k, 30k, and 50k vector datasets;
- 2048 dimensions;
- zvec, Qdrant sidecar, sqlite-vec, LanceDB, USearch, turbovec, and Chroma;
- delete, reopen, and basic concurrent read/write smoke checks.

Minimum datasets:

- 10k retrieval units;
- 100k retrieval units;
- 1M retrieval units if feasible;
- Chinese transcript-heavy material;
- mixed transcript + OCR + visual units;
- repeated item reindex/delete cycles.

Measure:

- cold open time;
- first search latency after app launch;
- indexing throughput;
- incremental item delete/rewrite time;
- top-k recall overlap with Qdrant;
- ranking quality after Cerul fusion/rerank;
- memory use during indexing and search;
- disk footprint;
- crash during write, then reopen;
- concurrent read while indexing;
- app packaging and signing on macOS ARM64/x64.

Remaining product-quality tests:

- real Cerul `retrieval_units` exported from local indexed videos;
- Chinese transcript-heavy queries;
- OCR-visible-text queries;
- image/keyframe branch queries;
- mixed API/local embedding profile switching;
- app interruption during bulk zvec writes, then restart;
- bundled Rust integration and notarized macOS packaging.

Pass/fail gates:

- no sidecar/process/port dependency;
- broken vector index can be repaired without touching SQLite facts;
- transcript search remains available when vectors are unavailable;
- startup does not block the whole app;
- search quality is comparable after shared reranking.

## Main Risks

1. **Project maturity**
   zvec is newer than Qdrant and LanceDB. API stability and edge-case recovery
   need direct validation.

2. **Crash recovery**
   WAL support is promising, but Cerul must test interruption during bulk
   writes, deletes, and app shutdown.

3. **Concurrency**
   Cerul needs stable reads while indexing writes are happening. Verify the
   supported concurrency model rather than assuming it matches SQLite. The first
   Python spike showed zvec allows reads/writes through the same handle, but not
   multiple simultaneously opened handles on the same collection path.

4. **Hybrid retrieval quality**
   If we use zvec FTS/hybrid features, compare Chinese and mixed OCR/transcript
   retrieval against the current SQLite FTS + vector fusion. Do not migrate both
   vector storage and lexical retrieval in the same first step.

5. **Packaging**
   Verify static/dynamic library requirements, code signing, notarization,
   universal builds, and CI availability.

6. **Index lifecycle**
   We need clear versioning and cleanup rules so stale local indexes do not pile
   up like old Qdrant collections.

## Recommendation

Proceed in two phases.

### Phase 1: zvec as vector-only backend

Use zvec only for vector top-k and minimal filters. Keep SQLite FTS and all
metadata in SQLite.

This directly addresses the Qdrant sidecar reliability problem while minimizing
product risk.

### Phase 2: Evaluate richer zvec hybrid features

Only after Phase 1 is stable, test whether zvec should also participate in
hybrid retrieval. Keep the current SQLite FTS path as the baseline and fallback.

If zvec fails the implementation spike, evaluate sqlite-vec as the conservative
fallback first and LanceDB as the richer data-layer alternative second. LanceDB
is likely stronger when the local vector layer needs to act more like a
multimodal data table or local dataset store. Current API/local embedding
profiles alone are not enough reason to move metadata ownership away from
SQLite.

## Open Questions

- Does zvec support the exact filter and delete patterns Cerul needs at the
  required scale?
- Can zvec handle app interruption during bulk writes without manual repair?
- What is the best collection/table layout for embedding profile and search
  index version isolation?
- Should migration from Qdrant re-embed from source, or export existing Qdrant
  vectors where possible?
- How much top-k overlap do we require before shared reranking?
- What UI should expose "Rebuild semantic index" without sounding destructive?
