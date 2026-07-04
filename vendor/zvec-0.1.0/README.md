# zvec-rs

[![CI](https://github.com/oly-wan-kenobi/zvec-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/oly-wan-kenobi/zvec-rs/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/zvec?label=docs.rs)](https://docs.rs/zvec)
[![crates.io](https://img.shields.io/crates/v/zvec.svg?label=crates.io)](https://crates.io/crates/zvec)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
![MSRV 1.75](https://img.shields.io/badge/MSRV-1.75-informational.svg)

Rust bindings to [zvec](https://github.com/alibaba/zvec), Alibaba's
lightweight, in-process vector database.

zvec-rs has two layers:

- **Raw FFI** in [`zvec::sys`](src/sys.rs), generated at build time by
  `bindgen` from a pinned copy of upstream's `c_api.h` (`vendor/c_api.h`).
- **Safe wrappers** at the crate root (and in submodules) that cover the
  full public C API — schemas, index parameters, collections, documents,
  queries, stats, and global configuration — with RAII `Drop`s and
  `Result<T, ZvecError>` on every fallible path.

The crate pins zvec **v0.3.1** — that is the version whose `c_api.h` is
vendored here, whose wheel the `bundled` feature downloads, and whose git
tag `scripts/build-zvec.sh` checks out by default.

---

## Contents

- [Quickstart](#quickstart)
- [Picking an install path](#picking-an-install-path)
  - [Option A: the `bundled` cargo feature](#option-a-the-bundled-cargo-feature)
  - [Option B: build `libzvec_c_api` from source](#option-b-build-libzvec_c_api-from-source)
  - [Option C: point at an existing install](#option-c-point-at-an-existing-install)
- [Environment variable reference](#environment-variable-reference)
- [Cargo features](#cargo-features)
- [Supported targets](#supported-targets)
- [API tour](#api-tour)
- [Thread safety](#thread-safety)
- [Running the examples and tests](#running-the-examples-and-tests)
- [Cookbook](#cookbook)
- [Comparison to `igobypenn/zvec-rust-binding`](#comparison-to-igobypennzvec-rust-binding)
- [Repository layout](#repository-layout)
- [Contributing](#contributing)
- [License](#license)

---

## Quickstart

```toml
[dependencies]
zvec = { version = "0.1", features = ["bundled"] }
```

```rust
use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams,
    IndexType, MetricType, VectorQuery,
};

fn main() -> zvec::Result<()> {
    // Build a schema: PK + text field with an inverted index, plus a 3-D
    // HNSW vector field.
    let mut schema = CollectionSchema::new("docs")?;

    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;
    let mut id = FieldSchema::new("id", DataType::String, false, 0)?;
    id.set_index_params(&invert)?;
    schema.add_field(&id)?;

    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(MetricType::Cosine)?;
    hnsw.set_hnsw_params(16, 200)?;
    let mut emb = FieldSchema::new("embedding", DataType::VectorFp32, false, 3)?;
    emb.set_index_params(&hnsw)?;
    schema.add_field(&emb)?;

    let collection = Collection::create_and_open("./my_coll", &schema, None)?;

    let mut doc = Doc::new()?;
    doc.set_pk("doc1")?;
    doc.add_string("id", "doc1")?;
    doc.add_vector_fp32("embedding", &[0.1, 0.2, 0.3])?;
    collection.insert(&[&doc])?;
    collection.flush()?;

    let mut q = VectorQuery::new()?;
    q.set_field_name("embedding")?;
    q.set_query_vector_fp32(&[0.1, 0.2, 0.3])?;
    q.set_topk(10)?;
    for row in collection.query(&q)?.iter() {
        println!("{:?} score={}", row.pk_copy(), row.score());
    }
    Ok(())
}
```

A faithful Rust port of zvec's own `examples/c/basic_example.c` lives at
[`examples/basic.rs`](examples/basic.rs).

---

## Picking an install path

zvec-rs **links against** a prebuilt `libzvec_c_api` — it does not try to
compile zvec from source in its own `build.rs` by default. Three supported
install paths, fastest-first:

| Path | When it's best | Time on a clean build | Network? | Platforms |
|---|---|---|---|---|
| **`bundled` feature** | Local dev, CI, anything where a small binary download is fine | ~30 s | Yes (PyPI) | Upstream wheel matrix |
| **`scripts/build-zvec.sh`** | Targets upstream doesn't ship a wheel for; stricter supply-chain needs | 20–30 min first time, cached afterwards | Yes (git clone + submodules, ~500 MB) | Any platform zvec compiles on |
| **External prebuilt** | You already have zvec installed | 0 s | No | Any |

### Option A: the `bundled` cargo feature

With `--features bundled`, `build.rs` downloads the pinned zvec PyPI wheel
for the current `TARGET`, verifies its SHA-256, and extracts
`libzvec_c_api` + `c_api.h` into `$OUT_DIR/zvec-bundled/`. The linker
search path and rpath are wired so the resulting binary finds the library
without any `LD_LIBRARY_PATH` dance.

```toml
[dependencies]
zvec = { version = "0.1", features = ["bundled"] }
```

```sh
cargo build --features bundled
cargo test  --features bundled
```

Escape hatches:

- `ZVEC_BUNDLED_WHEEL_URL` + `ZVEC_BUNDLED_WHEEL_SHA256` — override the pin
  (e.g. test a newer upstream release or use a local mirror).
- `ZVEC_BUNDLED_WHEEL_PATH` — skip the network entirely and point at a
  local `.whl` file. Useful for air-gapped or TLS-restricted environments.

If the target isn't in the wheel matrix, `build.rs` emits a `cargo:warning`
and falls through to Option C (i.e. `ZVEC_LIB_DIR` / `ZVEC_ROOT` /
`pkg-config` discovery).

### Option B: build `libzvec_c_api` from source

Use [`scripts/build-zvec.sh`](scripts/build-zvec.sh) to clone upstream's
repository at the matching tag, run CMake, and install a flat
`{lib,include}` prefix that the crate can consume:

```sh
./scripts/build-zvec.sh "$PWD/zvec-install"

export ZVEC_ROOT="$PWD/zvec-install"
export LD_LIBRARY_PATH="$ZVEC_ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo test
```

Requirements on the build host: `cmake ≥ 3.13`, `ninja` (or `make`), a
C++17 compiler, `git`, `patch`, and `libclang` (needed by bindgen when this
crate itself compiles). The build pulls in RocksDB, Apache Arrow, Protobuf,
glog, gflags, ANTLR, LZ4, CRoaring, and RaBitQ as git submodules and
compiles them all — budget 20–30 minutes for the first run.

Script overrides:

| Variable          | Default                              | Purpose                         |
|-------------------|--------------------------------------|---------------------------------|
| `ZVEC_REF`        | `v0.3.1`                             | Git ref to check out            |
| `ZVEC_REPO`       | `https://github.com/alibaba/zvec`    | Upstream repository URL         |
| `ZVEC_SRC_DIR`    | *(clone into `.zvec-build-work/`)*   | Use an existing checkout        |
| `ZVEC_BUILD_DIR`  | `.zvec-build-work/build`             | CMake build directory           |
| `CMAKE_GENERATOR` | `Unix Makefiles`                     | e.g. `Ninja`                    |
| `JOBS`            | `nproc`                              | Parallel build jobs             |

### Option C: point at an existing install

If zvec is already on the system (via a package manager, your own build, or
a prior run of Option A or B), hand `build.rs` a location:

```sh
# Flat install prefix: $ZVEC_ROOT/lib and $ZVEC_ROOT/include/zvec/c_api.h
export ZVEC_ROOT=/opt/zvec
cargo build

# Or just the library dir (header comes from vendor/c_api.h):
export ZVEC_LIB_DIR=/opt/zvec/lib
cargo build

# Or a pkg-config file:
cargo build --features pkg-config
```

---

## Environment variable reference

Recognised by `build.rs` on every build:

| Variable                     | Purpose                                                                 |
|------------------------------|-------------------------------------------------------------------------|
| `ZVEC_ROOT`                  | Install prefix (`lib/`, `include/zvec/`). Overrides headers + lib path. |
| `ZVEC_LIB_DIR`               | Directory containing `libzvec_c_api`.                                   |
| `ZVEC_INCLUDE_DIR`           | Directory containing `zvec/c_api.h`.                                    |
| `ZVEC_STATIC=1`              | Link `zvec_c_api` statically instead of as a shared library.            |
| `ZVEC_BUNDLED_WHEEL_URL`     | Custom wheel URL (feature: `bundled`). Requires the companion SHA.      |
| `ZVEC_BUNDLED_WHEEL_SHA256`  | Expected SHA-256 for the wheel at `ZVEC_BUNDLED_WHEEL_URL`.             |
| `ZVEC_BUNDLED_WHEEL_PATH`    | Local wheel file to use instead of downloading (feature: `bundled`).    |

---

## Cargo features

| Feature      | Effect                                                                                                                               |
|--------------|--------------------------------------------------------------------------------------------------------------------------------------|
| *(default)*  | Build the crate. Expect `ZVEC_ROOT` / `ZVEC_LIB_DIR` / pkg-config / a system-installed lib to provide `libzvec_c_api` at link time.  |
| `bundled`    | Download + extract the upstream PyPI wheel at build time. Pulls in `ureq`, `zip`, and `sha2` as build-dependencies.                  |
| `pkg-config` | Probe for a system `zvec_c_api.pc` after the env vars are consulted.                                                                 |

---

## Supported targets

`bundled` is pinned to the wheels upstream publishes (cp311 ABI, but the
extracted `libzvec_c_api` is Python-independent):

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

The crate itself — raw FFI + safe wrappers — is not target-specific. Any
platform that zvec's CMake build produces a `libzvec_c_api` for works via
Options B or C.

---

## API tour

The safe wrappers live in several modules, all re-exported at the crate
root.

- `zvec::Collection` — `create_and_open`, `open`, `close`, `flush`,
  `optimize`; DDL (`create_index`, `drop_index`, `add_column`, `drop_column`,
  `alter_column`); DML (`insert`, `update`, `upsert`, `delete` and the
  `*_with_results` variants + `delete_by_filter`); DQL (`query`, `fetch`)
  returning a `DocSet` that cleans up its zvec-allocated buffer on drop.
- `zvec::CollectionSchema` + `zvec::FieldSchema` (+ non-owning
  `FieldSchemaRef<'_>`) — schema creation, field manipulation, validation,
  field enumeration, and index DDL.
- `zvec::IndexParams` — typed setters for HNSW (`m`, `ef_construction`), IVF
  (`n_list`, `n_iters`, `use_soar`), and inverted (`enable_range_opt`,
  `enable_extended_wildcard`) index parameters.
- `zvec::{HnswQueryParams, IvfQueryParams, FlatQueryParams}` — query-side
  tuning (`ef`, `nprobe`, `radius`, `is_linear`, refiner), transferable into
  a query via `VectorQuery::set_*_params`.
- `zvec::{VectorQuery, GroupByVectorQuery}` — all fields/knobs from the C
  API, plus `set_query_vector_fp32` / `_fp64` for typed vector inputs.
- `zvec::HybridSearch` — runs N `VectorQuery`s and fuses the result
  lists with [`zvec::rerank::RrfReRanker`] (default) or
  [`zvec::rerank::WeightedReRanker`]. Cookbook example below.
- `zvec::Doc` (+ non-owning `DocRef<'_>`) — primary key and per-field
  setters (`add_string`, `add_float`, `add_vector_fp32`,
  `add_vector_int8`, `add_vector_fp16_bits`, `add_vector_int4_packed`,
  `add_vector_binary32`, `add_array_int32`, …) and matching `get_*`
  readers; `serialize` / `deserialize` / `validate` / `to_detail_string`.
- `zvec::IntoDoc` / `zvec::FromDoc` traits + `#[derive(IntoDoc)]` and
  `#[derive(FromDoc)]` (via the `derive` feature) — skip `Doc::new() +
  add_*` boilerplate on the write side and manual `get_*` decoding on
  the read side:
  ```rust
  #[derive(IntoDoc, FromDoc)]
  struct Article {
      #[zvec(pk)] id: String,
      title: String,
      #[zvec(vector_fp32)] embedding: Vec<f32>,
      summary: Option<String>,
  }
  let doc = article.into_doc()?;
  collection.insert(&[&doc])?;

  let results = collection.fetch(&["a"])?;
  let got: Article = Article::from_doc(results.get(0).unwrap())?;
  ```
  Attributes: `pk`, `skip`, `rename = "..."`, plus type hints for
  `Vec`-typed fields (`binary`, `vector_fp32`, `vector_fp64`,
  `vector_int8`, `vector_int16`). `Option<T>` is supported: on the
  write side `None` emits a null field; on the read side a missing or
  null field decodes as `None`.
- `zvec::{DataType, IndexType, MetricType, QuantizeType, LogLevel,
  LogType, DocOperator}` — strongly-typed mirrors of the C `typedef`s and
  `#define`s, with an `Other(u32)` escape hatch for values not recognised
  by this bindings version.
- `zvec::{CollectionOptions, CollectionStats}` — options builder and the
  stats snapshot returned by `Collection::stats`.
- `zvec::{Config, LogConfig, initialize, shutdown, is_initialized}` —
  optional global configuration passed to `initialize`; not required for
  basic usage.
- `zvec::{ErrorCode, ZvecError, Result, clear_last_error}` — errors carry
  the last-error message from the C API when one is set.
- `zvec::{version, version_major, version_minor, version_patch,
  check_version}` — runtime version from the linked zvec library.

All owning wrappers implement `Drop`.

## Thread safety

Every wrapper has an explicit `unsafe impl` with a `SAFETY:` note.

- **`Send + Sync`** (pure builder or read-only snapshot): `Config`,
  `LogConfig`, `IndexParams`, `HnswQueryParams`, `IvfQueryParams`,
  `FlatQueryParams`, `FieldSchema`, `FieldSchemaRef<'_>`,
  `CollectionSchema`, `CollectionOptions`, `CollectionStats`,
  `DocRef<'_>`, `Collection`.
- **`Send` only** (mutable C-side state without documented thread-safe
  reads): `Doc`, `VectorQuery`, `GroupByVectorQuery`, `DocSet`.

---

## Running the examples and tests

```sh
# Print the runtime version reported by the linked zvec.
cargo run --example version

# Rust port of basic_example.c — creates a collection in $TMPDIR.
cargo run --example basic

# End-to-end integration tests (5 tests + doctest).
cargo test
```

All three need `libzvec_c_api` available — either via `--features bundled`,
`ZVEC_ROOT` pointing at a source build, or an external install. With
`--features bundled` the resulting binary has an rpath baked in and needs
no runtime env vars.

---

## Cookbook

End-to-end recipes you can `cargo run` directly. All assume
`--features bundled` (or one of the install paths above) and live under
[`examples/`](examples/).

### Semantic search

[`examples/semantic_search.rs`](examples/semantic_search.rs) — index a
small corpus of text + 4-D embeddings, then query nearest neighbours by
cosine similarity.

```sh
cargo run --example semantic_search --features bundled
```

### Hybrid search across multiple embeddings

[`examples/hybrid_search.rs`](examples/hybrid_search.rs) — every doc
carries a `title_emb` and a `body_emb`; a query against each is fused
with Reciprocal Rank Fusion via [`HybridSearch`](src/hybrid.rs).

```rust
let hits = HybridSearch::new()
    .query(title_q)
    .query(body_q)
    .top_k(10)
    .execute(&collection)?;
```

```sh
cargo run --example hybrid_search --features bundled
```

### JSON ingestion

[`examples/json_ingest.rs`](examples/json_ingest.rs) — feed
`serde_json::Value`s straight into the collection through
`Doc::from_json`. Field types are resolved through the schema so the
caller doesn't manually pick `add_string` vs `add_vector_fp32`.

```sh
cargo run --example json_ingest --features "bundled serde-json"
```

### Manual rerank

`zvec::rerank::{RrfReRanker, WeightedReRanker}` are usable
stand-alone — pass any `Vec<Hit>` you produce, from any source, not
just `Collection::query`.

---

## Comparison to [`igobypenn/zvec-rust-binding`](https://github.com/igobypenn/zvec-rust-binding)

Both crates are Rust bindings to zvec, but they were designed in different
zvec generations and take different architectural bets.

| | **`zvec-rs` (this crate)** | **`igobypenn/zvec-rust-binding`** |
|---|---|---|
| Upstream zvec version pinned | `v0.3.1` | `v0.2.1` (pre-official-C-API) |
| FFI boundary | `bindgen` at upstream's own `c_api.h` (the C API added in zvec 0.3.0) | Hand-rolled `zvec-c-wrapper/` on top of zvec's C++ libs, then `bindgen` at that |
| Crate shape | Single crate `zvec`, `sys` as a submodule | Workspace: `zvec-sys` + `zvec-bindings` + `zvec-c-wrapper` |
| Default build behaviour | `cargo build` only runs `bindgen` — linking expects a prebuilt `libzvec_c_api` | `cargo build` downloads zvec source (~500 MB) and runs CMake; 5–15 min first time, cached |
| Zero-setup option | `--features bundled` (47 MB wheel download, ~30 s) | *same* as the default — always compiles zvec |
| Hand-rolled C shim | Not needed (upstream ships one) | Required (the pinned 0.2.1 predates the official one) |
| Libs named at link time | One thing: `zvec_c_api` | `zvec_db` / `_core` + Arrow, Parquet, Boost, RocksDB, stdc++, pthread |
| `Send` / `Sync` | Explicit per-type `unsafe impl` with `SAFETY:` notes | Opt-in `sync` feature providing a `SharedCollection: Arc<…>` |
| Env-var overrides | `ZVEC_ROOT`, `ZVEC_LIB_DIR`, `ZVEC_INCLUDE_DIR`, `ZVEC_STATIC`, `ZVEC_BUNDLED_WHEEL_*` (describe where to *find* the lib) | `ZVEC_BUILD_TYPE`, `ZVEC_BUILD_PARALLEL`, `ZVEC_CPU_ARCH`, `ZVEC_OPENMP` (drive the *in-crate* CMake build) |
| Extra high-level types | Typed `add_*` / `get_*` for every vector + array data type | Re-rankers (`RrfReRanker`, `WeightedReRanker`) |

Headline difference: the other crate predates zvec's upstream C API, so it
carries its own C shim and drags a full CMake build into every consumer's
`cargo build`. That's more turnkey out of the box (one command, and you
don't think about a shared library), but it also means:

- First-time compile is 5–15 min unconditionally; zvec-rs with `bundled`
  is ~30 s.
- The shim enumerates zvec's transitive C++ dependencies by hand in its
  linker config.
- Upgrading zvec means updating a handwritten wrapper in lockstep with
  internal zvec C++ headers.

zvec-rs instead targets zvec's own `c_api.h` — a single
`libzvec_c_api.so` that already bundles all of that — and offers three
distinct install paths so you can pick your tradeoff: `bundled` for
speed, source build for reproducibility, or external prebuilt for zero
overhead. If you want re-rankers or you're stuck on zvec 0.2.x,
`igobypenn/zvec-rust-binding` is still the right choice today.

---

## Repository layout

```
.
├── vendor/c_api.h              # Pinned upstream C API (zvec v0.3.1).
├── build.rs                    # bindgen + linker discovery; bundled-feature
│                                 wheel fetch + extract.
├── scripts/build-zvec.sh       # Reproducible source build helper.
├── src/
│   ├── sys.rs                  # `include!` of bindgen output.
│   ├── lib.rs                  # Crate root + re-exports + doctest.
│   ├── collection.rs           # Collection + DocSet + WriteSummary/Result.
│   ├── doc.rs                  # Doc / DocRef + typed add_*/get_*.
│   ├── schema.rs               # FieldSchema(/Ref) + CollectionSchema.
│   ├── query.rs                # VectorQuery + GroupByVectorQuery.
│   ├── query_params.rs         # HnswQueryParams / IvfQueryParams / FlatQueryParams.
│   ├── index_params.rs         # IndexParams.
│   ├── options.rs              # CollectionOptions.
│   ├── stats.rs                # CollectionStats.
│   ├── config.rs               # Config / LogConfig / initialize / shutdown.
│   ├── types.rs                # DataType, IndexType, MetricType, ...
│   ├── error.rs                # ErrorCode, ZvecError, Result, check().
│   ├── version.rs              # version() / version_major() / ...
│   └── ffi_util.rs             # cstring(), slice_as_bytes(), etc.
├── examples/
│   ├── version.rs              # Prints the runtime zvec version.
│   └── basic.rs                # Rust port of basic_example.c.
├── tests/integration.rs        # 5 end-to-end roundtrip tests.
└── .github/workflows/
    ├── ci.yml                  # Per-PR: bundled-feature matrix
    │                           # (Linux + macOS) running rustfmt,
    │                           # clippy, and tests against an
    │                           # upstream-shipped libzvec_c_api.
    └── source-build.yml        # Weekly cron + manual dispatch:
                                # validates `scripts/build-zvec.sh`
                                # by compiling zvec from source and
                                # re-running the test suite.
```

## Contributing

- `cargo fmt --all` and `cargo clippy --all-targets --no-deps -- -D warnings`
  are both enforced in CI; please run them locally before opening a PR.
- The integration tests in `tests/integration.rs` exercise the library
  end-to-end — they need a working `libzvec_c_api`. The `bundled` feature is
  the lowest-friction way to get one: `cargo test --features bundled`.
- When bumping the vendored zvec version, update `vendor/c_api.h`,
  `ZVEC_REF` in `scripts/build-zvec.sh`, both workflow files in
  `.github/workflows/`, and the pinned wheels in `build.rs`.

## License

Apache-2.0, matching upstream zvec.
