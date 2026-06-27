#!/usr/bin/env python3
"""Local vector-store benchmark for Cerul desktop storage candidates.

This is intentionally product-shaped rather than leaderboard-shaped:
it uses Cerul's local embedding dimensionality, records CRUD/reopen behavior,
and keeps metadata ownership outside the vector store.
"""

from __future__ import annotations

import argparse
import contextlib
import csv
import gc
import importlib
import json
import math
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

import numpy as np
import psutil

os.environ.setdefault("RUST_LOG", "error")


BatchSearch = Callable[[np.ndarray, int], list[list[int]]]


@dataclass
class BackendResult:
    backend: str
    status: str = "ok"
    n: int = 0
    dim: int = 0
    queries: int = 0
    build_s: float | None = None
    query_total_s: float | None = None
    query_avg_ms: float | None = None
    query_p50_ms: float | None = None
    query_p95_ms: float | None = None
    recall_at_k: float | None = None
    delete_ok: bool | None = None
    reopen_ok: bool | None = None
    concurrent_read_errors: int | None = None
    concurrent_write_ok: bool | None = None
    disk_bytes: int | None = None
    rss_delta_bytes: int | None = None
    import_version: str | None = None
    package_bytes: int | None = None
    notes: list[str] = field(default_factory=list)
    error: str | None = None

    def to_dict(self) -> dict[str, Any]:
        data = self.__dict__.copy()
        data["notes"] = "; ".join(self.notes)
        return data


@dataclass
class BenchContext:
    vectors: np.ndarray
    queries: np.ndarray
    ground_truth: np.ndarray
    workdir: Path
    k: int
    delete_count: int
    qdrant_bin: Path | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--n", type=int, default=10_000)
    parser.add_argument("--dim", type=int, default=2_048)
    parser.add_argument("--queries", type=int, default=100)
    parser.add_argument("--k", type=int, default=10)
    parser.add_argument("--seed", type=int, default=20260627)
    parser.add_argument(
        "--backends",
        default="zvec,lancedb,sqlite_vec,usearch,turbovec,qdrant_sidecar,chroma",
        help="Comma-separated backend names",
    )
    parser.add_argument(
        "--out-dir",
        default=".artifacts/vector-db-bench/latest",
    )
    parser.add_argument(
        "--qdrant-bin",
        default="third-party/aarch64-apple-darwin/qdrant",
    )
    return parser.parse_args()


def normalize(vectors: np.ndarray) -> np.ndarray:
    norms = np.linalg.norm(vectors, axis=1, keepdims=True)
    norms[norms == 0] = 1
    return (vectors / norms).astype(np.float32, copy=False)


def generate_dataset(n: int, dim: int, q: int, seed: int, k: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(seed)
    vectors = normalize(rng.standard_normal((n, dim), dtype=np.float32))

    # Product-shaped queries: most are noisy variants of existing vectors, which
    # simulates a query embedding landing near a relevant transcript/OCR unit.
    source_ids = rng.integers(0, n, size=q)
    noise = rng.standard_normal((q, dim), dtype=np.float32) * 0.03
    queries = normalize(vectors[source_ids] + noise)

    scores = queries @ vectors.T
    ground_truth = np.argpartition(-scores, kth=np.arange(k), axis=1)[:, :k]
    gt_scores = np.take_along_axis(scores, ground_truth, axis=1)
    order = np.argsort(-gt_scores, axis=1)
    ground_truth = np.take_along_axis(ground_truth, order, axis=1).astype(np.int64)
    return vectors, queries, ground_truth


def recall_at_k(results: list[list[int]], truth: np.ndarray, k: int) -> float:
    total = 0
    for got, expected in zip(results, truth, strict=True):
        total += len(set(got[:k]).intersection(int(x) for x in expected[:k]))
    return total / (len(results) * k)


def percentile(values: list[float], p: float) -> float:
    if not values:
        return math.nan
    return float(np.percentile(np.array(values, dtype=np.float64), p))


def dir_size(path: Path) -> int:
    if not path.exists():
        return 0
    total = 0
    if path.is_file():
        return path.stat().st_size
    for root, _, files in os.walk(path):
        for name in files:
            with contextlib.suppress(OSError):
                total += (Path(root) / name).stat().st_size
    return total


def module_version_and_size(module_name: str) -> tuple[str | None, int | None]:
    try:
        mod = importlib.import_module(module_name)
    except Exception:
        return None, None
    version = getattr(mod, "__version__", None)
    path = getattr(mod, "__file__", None)
    if not path:
        return version, None
    root = Path(path).parent
    return str(version) if version is not None else None, dir_size(root)


def rss_bytes() -> int:
    return psutil.Process().memory_info().rss


def batch_iter(n: int, batch_size: int) -> range:
    return range(0, n, batch_size)


def timed_query(search: BatchSearch, queries: np.ndarray, k: int) -> tuple[list[list[int]], list[float]]:
    results: list[list[int]] = []
    latencies: list[float] = []
    for query in queries:
        started = time.perf_counter()
        hits = search(query[None, :], k)[0]
        latencies.append(time.perf_counter() - started)
        results.append(hits)
    return results, latencies


def run_common_checks(
    result: BackendResult,
    search: BatchSearch,
    queries: np.ndarray,
    k: int,
    deleted_ids: set[int],
    reopen: Callable[[], BatchSearch] | None = None,
    concurrent_write: Callable[[], None] | None = None,
) -> None:
    if deleted_ids:
        hits = search(queries[: min(10, len(queries))], max(k, 20))
        result.delete_ok = all(not deleted_ids.intersection(row[:k]) for row in hits)

    if reopen is not None:
        try:
            reopened_search = reopen()
            reopened = reopened_search(queries[: min(10, len(queries))], k)
            result.reopen_ok = len(reopened) == min(10, len(queries)) and all(len(row) > 0 for row in reopened)
        except Exception as exc:  # noqa: BLE001
            result.reopen_ok = False
            result.notes.append(f"reopen failed: {type(exc).__name__}: {exc}")

    errors: list[str] = []

    def reader() -> None:
        try:
            for _ in range(10):
                _ = search(queries[:5], k)
        except Exception as exc:  # noqa: BLE001
            errors.append(f"{type(exc).__name__}: {exc}")

    threads = [threading.Thread(target=reader) for _ in range(4)]
    for thread in threads:
        thread.start()
    if concurrent_write is not None:
        try:
            concurrent_write()
            result.concurrent_write_ok = True
        except Exception as exc:  # noqa: BLE001
            result.concurrent_write_ok = False
            result.notes.append(f"concurrent write failed: {type(exc).__name__}: {exc}")
    for thread in threads:
        thread.join()
    result.concurrent_read_errors = len(errors)
    if errors:
        result.notes.append("concurrent read errors: " + " | ".join(errors[:3]))


def bench_zvec(ctx: BenchContext) -> BackendResult:
    import zvec

    result = BackendResult("zvec", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("zvec")
    path = ctx.workdir / "zvec_collection"
    schema = zvec.CollectionSchema(
        "cerul_vector_bench",
        fields=[
            zvec.FieldSchema("item_id", zvec.DataType.INT64),
            zvec.FieldSchema("unit_kind", zvec.DataType.STRING),
        ],
        vectors=[
            zvec.VectorSchema(
                "vector",
                zvec.DataType.VECTOR_FP32,
                dimension=ctx.vectors.shape[1],
                index_param=zvec.HnswIndexParam(
                    metric_type=zvec.MetricType.COSINE,
                    m=16,
                    ef_construction=200,
                ),
            )
        ],
    )
    before = rss_bytes()
    started = time.perf_counter()
    collection = zvec.create_and_open(str(path), schema)
    for start in batch_iter(len(ctx.vectors), 256):
        stop = min(start + 256, len(ctx.vectors))
        docs = [
            zvec.Doc(
                id=str(i),
                fields={"item_id": int(i // 8), "unit_kind": "transcript"},
                vectors={"vector": ctx.vectors[i]},
            )
            for i in range(start, stop)
        ]
        collection.insert(docs)
    collection.flush()
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        out: list[list[int]] = []
        param = zvec.HnswQueryParam(ef=200)
        for q in qs:
            docs = collection.query(zvec.Query("vector", vector=q, param=param), topk=k)
            out.append([int(doc.id) for doc in docs])
        return out

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)

    delete_ids = set(range(ctx.delete_count))
    collection.delete([str(i) for i in delete_ids])
    collection.flush()

    def reopen() -> BatchSearch:
        reopened = zvec.open(str(path))

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            out: list[list[int]] = []
            for q in qs:
                docs = reopened.query(zvec.Query("vector", vector=q, param=zvec.HnswQueryParam(ef=200)), topk=k)
                out.append([int(doc.id) for doc in docs])
            return out

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        docs = [
            zvec.Doc(id=str(base + i), fields={"item_id": base + i, "unit_kind": "transcript"}, vectors={"vector": ctx.vectors[i]})
            for i in range(16)
        ]
        collection.upsert(docs)
        collection.flush()

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, None, concurrent_write)
    del collection
    gc.collect()
    try:
        reopened_search = reopen()
        reopened = reopened_search(ctx.queries[: min(10, len(ctx.queries))], ctx.k)
        result.reopen_ok = len(reopened) == min(10, len(ctx.queries)) and all(len(row) > 0 for row in reopened)
    except Exception as exc:  # noqa: BLE001
        result.reopen_ok = False
        result.notes.append(f"post-close reopen failed: {type(exc).__name__}: {exc}")
    result.disk_bytes = dir_size(path)
    return result


def bench_lancedb(ctx: BenchContext) -> BackendResult:
    import lancedb
    import pyarrow as pa

    result = BackendResult("lancedb", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("lancedb")
    path = ctx.workdir / "lancedb"
    before = rss_bytes()
    started = time.perf_counter()
    db = lancedb.connect(str(path))
    ids = np.arange(len(ctx.vectors), dtype=np.int64)
    vectors = pa.FixedSizeListArray.from_arrays(pa.array(ctx.vectors.ravel(), type=pa.float32()), ctx.vectors.shape[1])
    table_data = pa.table(
        {
            "id": pa.array(ids),
            "item_id": pa.array(ids // 8),
            "unit_kind": pa.array(["transcript"] * len(ctx.vectors)),
            "vector": vectors,
        }
    )
    table = db.create_table("vectors", data=table_data, mode="overwrite")
    result.notes.append("exact search unless LanceDB auto-selects an index")
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        out: list[list[int]] = []
        for q in qs:
            rows = table.search(q).metric("cosine").limit(k).select(["id", "_distance"]).to_list()
            out.append([int(row["id"]) for row in rows])
        return out

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    table.delete("id >= 0 AND id < " + str(ctx.delete_count))

    def reopen() -> BatchSearch:
        reopened_db = lancedb.connect(str(path))
        reopened_table = reopened_db.open_table("vectors")

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            return [
                [
                    int(row["id"])
                    for row in reopened_table.search(q).metric("cosine").limit(k).select(["id", "_distance"]).to_list()
                ]
                for q in qs
            ]

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        add_vectors = pa.FixedSizeListArray.from_arrays(pa.array(ctx.vectors[:16].ravel(), type=pa.float32()), ctx.vectors.shape[1])
        table.add(
            pa.table(
                {
                    "id": pa.array(np.arange(base, base + 16, dtype=np.int64)),
                    "item_id": pa.array(np.arange(base, base + 16, dtype=np.int64)),
                    "unit_kind": pa.array(["transcript"] * 16),
                    "vector": add_vectors,
                }
            )
        )

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    result.disk_bytes = dir_size(path)
    return result


def bench_lancedb_ivf_hnsw_pq(ctx: BenchContext) -> BackendResult:
    import lancedb
    import pyarrow as pa

    result = BackendResult("lancedb_ivf_hnsw_pq", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("lancedb")
    path = ctx.workdir / "lancedb_ivf_hnsw_pq"
    before = rss_bytes()
    started = time.perf_counter()
    db = lancedb.connect(str(path))
    ids = np.arange(len(ctx.vectors), dtype=np.int64)
    vectors = pa.FixedSizeListArray.from_arrays(pa.array(ctx.vectors.ravel(), type=pa.float32()), ctx.vectors.shape[1])
    table = db.create_table(
        "vectors",
        data=pa.table(
            {
                "id": pa.array(ids),
                "item_id": pa.array(ids // 8),
                "unit_kind": pa.array(["transcript"] * len(ctx.vectors)),
                "vector": vectors,
            }
        ),
        mode="overwrite",
    )
    partitions = min(256, max(16, int(math.sqrt(len(ctx.vectors)))))
    sub_vectors = 64 if ctx.vectors.shape[1] >= 64 and ctx.vectors.shape[1] % 64 == 0 else 16
    table.create_index(
        metric="cosine",
        index_type="IVF_HNSW_PQ",
        num_partitions=partitions,
        num_sub_vectors=sub_vectors,
        vector_column_name="vector",
        replace=True,
        m=20,
        ef_construction=300,
    )
    result.notes.append(f"IVF_HNSW_PQ nprobes=32 refine_factor=20 partitions={partitions} sub_vectors={sub_vectors}")
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        out: list[list[int]] = []
        for q in qs:
            rows = (
                table.search(q)
                .metric("cosine")
                .nprobes(32)
                .refine_factor(20)
                .limit(k)
                .select(["id", "_distance"])
                .to_list()
            )
            out.append([int(row["id"]) for row in rows])
        return out

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    table.delete("id >= 0 AND id < " + str(ctx.delete_count))

    def reopen() -> BatchSearch:
        reopened_db = lancedb.connect(str(path))
        reopened_table = reopened_db.open_table("vectors")

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            return [
                [
                    int(row["id"])
                    for row in (
                        reopened_table.search(q)
                        .metric("cosine")
                        .nprobes(32)
                        .refine_factor(20)
                        .limit(k)
                        .select(["id", "_distance"])
                        .to_list()
                    )
                ]
                for q in qs
            ]

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        add_vectors = pa.FixedSizeListArray.from_arrays(pa.array(ctx.vectors[:16].ravel(), type=pa.float32()), ctx.vectors.shape[1])
        table.add(
            pa.table(
                {
                    "id": pa.array(np.arange(base, base + 16, dtype=np.int64)),
                    "item_id": pa.array(np.arange(base, base + 16, dtype=np.int64)),
                    "unit_kind": pa.array(["transcript"] * 16),
                    "vector": add_vectors,
                }
            )
        )

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    result.disk_bytes = dir_size(path)
    return result


def bench_sqlite_vec(ctx: BenchContext) -> BackendResult:
    import sqlite3

    import sqlite_vec

    result = BackendResult("sqlite_vec", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("sqlite_vec")
    path = ctx.workdir / "sqlite_vec.db"
    before = rss_bytes()
    started = time.perf_counter()
    conn = sqlite3.connect(path, check_same_thread=False)
    conn.enable_load_extension(True)
    sqlite_vec.load(conn)
    conn.execute(f"CREATE VIRTUAL TABLE vec_items USING vec0(vector float[{ctx.vectors.shape[1]}])")
    for start in batch_iter(len(ctx.vectors), 512):
        stop = min(start + 512, len(ctx.vectors))
        conn.executemany(
            "INSERT INTO vec_items(rowid, vector) VALUES (?, ?)",
            [(i, sqlite_vec.serialize_float32(ctx.vectors[i])) for i in range(start, stop)],
        )
    conn.commit()
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before
    result.notes.append("exact scan SQLite extension")

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        out: list[list[int]] = []
        for q in qs:
            rows = conn.execute(
                "SELECT rowid FROM vec_items WHERE vector MATCH ? AND k = ? ORDER BY distance",
                [sqlite_vec.serialize_float32(q), k],
            ).fetchall()
            out.append([int(row[0]) for row in rows])
        return out

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    conn.executemany("DELETE FROM vec_items WHERE rowid = ?", [(i,) for i in delete_ids])
    conn.commit()

    def reopen() -> BatchSearch:
        reopened = sqlite3.connect(path)
        reopened.enable_load_extension(True)
        sqlite_vec.load(reopened)

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            return [
                [
                    int(row[0])
                    for row in reopened.execute(
                        "SELECT rowid FROM vec_items WHERE vector MATCH ? AND k = ? ORDER BY distance",
                        [sqlite_vec.serialize_float32(q), k],
                    ).fetchall()
                ]
                for q in qs
            ]

        return reopened_search

    def concurrent_write() -> None:
        writer = sqlite3.connect(path)
        writer.enable_load_extension(True)
        sqlite_vec.load(writer)
        base = len(ctx.vectors)
        writer.executemany(
            "INSERT OR REPLACE INTO vec_items(rowid, vector) VALUES (?, ?)",
            [(base + i, sqlite_vec.serialize_float32(ctx.vectors[i])) for i in range(16)],
        )
        writer.commit()
        writer.close()

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    conn.close()
    result.disk_bytes = dir_size(path)
    return result


def run_usearch(ctx: BenchContext, backend_name: str, expansion_search: int, exact: bool) -> BackendResult:
    from usearch.index import Index

    result = BackendResult(backend_name, n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("usearch")
    path = ctx.workdir / f"{backend_name}.index"
    before = rss_bytes()
    started = time.perf_counter()
    index = Index(
        ndim=ctx.vectors.shape[1],
        metric="cos",
        connectivity=16,
        expansion_add=128,
        expansion_search=expansion_search,
    )
    index.add(np.arange(len(ctx.vectors), dtype=np.uint64), ctx.vectors, threads=0)
    index.save(path)
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before
    result.notes.append(
        f"index engine only; metadata/transactions remain external; expansion_search={expansion_search}; exact={exact}"
    )

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        matches = index.search(qs, k, threads=0, exact=exact)
        keys = matches.keys
        if keys.ndim == 1:
            return [[int(x) for x in keys]]
        return [[int(x) for x in row] for row in keys]

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    index.remove(np.array(list(delete_ids), dtype=np.uint64), compact=False)
    index.save(path)

    def reopen() -> BatchSearch:
        reopened = Index.restore(path)

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            matches = reopened.search(qs, k, exact=exact)
            keys = matches.keys
            if keys.ndim == 1:
                return [[int(x) for x in keys]]
            return [[int(x) for x in row] for row in keys]

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        index.add(np.arange(base, base + 16, dtype=np.uint64), ctx.vectors[:16])

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    result.disk_bytes = dir_size(path)
    return result


def bench_usearch(ctx: BenchContext) -> BackendResult:
    return run_usearch(ctx, "usearch", expansion_search=128, exact=False)


def bench_usearch_high_recall(ctx: BenchContext) -> BackendResult:
    return run_usearch(ctx, "usearch_high_recall", expansion_search=2048, exact=False)


def bench_usearch_exact(ctx: BenchContext) -> BackendResult:
    return run_usearch(ctx, "usearch_exact", expansion_search=2048, exact=True)


def bench_turbovec(ctx: BenchContext) -> BackendResult:
    import turbovec

    result = BackendResult("turbovec", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("turbovec")
    path = ctx.workdir / "turbovec.index"
    before = rss_bytes()
    started = time.perf_counter()
    index = turbovec.IdMapIndex(dim=ctx.vectors.shape[1], bit_width=4)
    index.add_with_ids(ctx.vectors, np.arange(len(ctx.vectors), dtype=np.uint64))
    index.prepare()
    index.write(str(path))
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before
    result.notes.append("4-bit quantized index engine; no scalar metadata layer")

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        _, ids = index.search(qs, k)
        return [[int(x) for x in row] for row in ids]

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    for i in delete_ids:
        with contextlib.suppress(Exception):
            index.remove(int(i))
    index.write(str(path))

    def reopen() -> BatchSearch:
        reopened = turbovec.IdMapIndex.load(str(path))

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            _, ids = reopened.search(qs, k)
            return [[int(x) for x in row] for row in ids]

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        index.add_with_ids(ctx.vectors[:16], np.arange(base, base + 16, dtype=np.uint64))
        index.prepare()

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    result.disk_bytes = dir_size(path)
    return result


def free_port() -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def wait_http(url: str, timeout_s: float) -> None:
    import requests

    deadline = time.time() + timeout_s
    last_error = None
    while time.time() < deadline:
        try:
            response = requests.get(url, timeout=1)
            if response.status_code < 500:
                return
        except Exception as exc:  # noqa: BLE001
            last_error = exc
        time.sleep(0.25)
    raise RuntimeError(f"{url} did not become ready: {last_error}")


@contextlib.contextmanager
def without_proxy_env():
    keys = [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ]
    old = {key: os.environ.get(key) for key in keys}
    for key in keys:
        os.environ.pop(key, None)
    try:
        yield
    finally:
        for key, value in old.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def bench_qdrant_sidecar(ctx: BenchContext) -> BackendResult:
    from qdrant_client import QdrantClient, models

    result = BackendResult("qdrant_sidecar", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("qdrant_client")
    if ctx.qdrant_bin is None or not ctx.qdrant_bin.exists():
        result.status = "skipped"
        result.error = f"qdrant binary not found: {ctx.qdrant_bin}"
        return result
    result.package_bytes = ctx.qdrant_bin.stat().st_size
    port = free_port()
    grpc_port = free_port()
    storage = ctx.workdir / "qdrant_storage"
    storage.mkdir(parents=True, exist_ok=True)
    log = (ctx.workdir / "qdrant.log").open("wb")
    env = os.environ.copy()
    env.update(
        {
            "QDRANT__STORAGE__STORAGE_PATH": str(storage),
            "QDRANT__STORAGE__SNAPSHOTS_PATH": str(storage / "snapshots"),
            "QDRANT__SERVICE__HTTP_PORT": str(port),
            "QDRANT__SERVICE__GRPC_PORT": str(grpc_port),
            "QDRANT__LOG_LEVEL": "WARN",
            "QDRANT__TELEMETRY_DISABLED": "true",
        }
    )
    start_sidecar = time.perf_counter()
    proc = subprocess.Popen([str(ctx.qdrant_bin)], cwd=storage, stdout=log, stderr=subprocess.STDOUT, env=env)
    try:
        wait_http(f"http://127.0.0.1:{port}/collections", 45)
        result.notes.append(f"sidecar_ready_s={time.perf_counter() - start_sidecar:.3f}")
        with without_proxy_env():
            client = QdrantClient(url=f"http://127.0.0.1:{port}", timeout=30)
            before = rss_bytes()
            started = time.perf_counter()
            collection_name = "bench_vectors"
            client.create_collection(
                collection_name,
                vectors_config=models.VectorParams(size=ctx.vectors.shape[1], distance=models.Distance.COSINE),
                hnsw_config=models.HnswConfigDiff(m=16, ef_construct=200),
            )
            for start in batch_iter(len(ctx.vectors), 256):
                stop = min(start + 256, len(ctx.vectors))
                points = [
                    models.PointStruct(
                        id=int(i),
                        vector=ctx.vectors[i].tolist(),
                        payload={"item_id": int(i // 8), "unit_kind": "transcript"},
                    )
                    for i in range(start, stop)
                ]
                client.upsert(collection_name, points=points, wait=True)
            result.build_s = time.perf_counter() - started
            result.rss_delta_bytes = rss_bytes() - before

            def search(qs: np.ndarray, k: int) -> list[list[int]]:
                out: list[list[int]] = []
                for q in qs:
                    rows = client.query_points(
                        collection_name,
                        query=q,
                        limit=k,
                        with_payload=False,
                        search_params=models.SearchParams(hnsw_ef=200),
                    ).points
                    out.append([int(point.id) for point in rows])
                return out

            hits, latencies = timed_query(search, ctx.queries, ctx.k)
            result.query_total_s = sum(latencies)
            result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
            result.query_p50_ms = 1000 * percentile(latencies, 50)
            result.query_p95_ms = 1000 * percentile(latencies, 95)
            result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
            delete_ids = set(range(ctx.delete_count))
            client.delete(collection_name, models.PointIdsList(points=list(delete_ids)), wait=True)

            def reopen() -> BatchSearch:
                reopened = QdrantClient(url=f"http://127.0.0.1:{port}", timeout=30)

                def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
                    return [
                        [
                            int(point.id)
                            for point in reopened.query_points(collection_name, query=q, limit=k, with_payload=False).points
                        ]
                        for q in qs
                    ]

                return reopened_search

            def concurrent_write() -> None:
                base = len(ctx.vectors)
                points = [
                    models.PointStruct(id=int(base + i), vector=ctx.vectors[i].tolist(), payload={"item_id": int(base + i)})
                    for i in range(16)
                ]
                client.upsert(collection_name, points=points, wait=True)

            run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    finally:
        with contextlib.suppress(Exception):
            proc.send_signal(signal.SIGTERM)
            proc.wait(timeout=10)
        with contextlib.suppress(Exception):
            proc.kill()
        log.close()
    result.disk_bytes = dir_size(storage)
    return result


def bench_chroma(ctx: BenchContext) -> BackendResult:
    import chromadb

    result = BackendResult("chroma", n=len(ctx.vectors), dim=ctx.vectors.shape[1], queries=len(ctx.queries))
    result.import_version, result.package_bytes = module_version_and_size("chromadb")
    path = ctx.workdir / "chroma"
    before = rss_bytes()
    started = time.perf_counter()
    client = chromadb.PersistentClient(path=str(path))
    collection = client.create_collection("bench_vectors", metadata={"hnsw:space": "cosine"})
    for start in batch_iter(len(ctx.vectors), 512):
        stop = min(start + 512, len(ctx.vectors))
        collection.add(
            ids=[str(i) for i in range(start, stop)],
            embeddings=ctx.vectors[start:stop].tolist(),
            metadatas=[{"item_id": int(i // 8), "unit_kind": "transcript"} for i in range(start, stop)],
        )
    result.build_s = time.perf_counter() - started
    result.rss_delta_bytes = rss_bytes() - before
    result.notes.append("persistent Python client; production docs recommend server-backed Chroma")

    def search(qs: np.ndarray, k: int) -> list[list[int]]:
        response = collection.query(query_embeddings=qs.tolist(), n_results=k, include=[])
        return [[int(x) for x in row] for row in response["ids"]]

    hits, latencies = timed_query(search, ctx.queries, ctx.k)
    result.query_total_s = sum(latencies)
    result.query_avg_ms = 1000 * result.query_total_s / len(latencies)
    result.query_p50_ms = 1000 * percentile(latencies, 50)
    result.query_p95_ms = 1000 * percentile(latencies, 95)
    result.recall_at_k = recall_at_k(hits, ctx.ground_truth, ctx.k)
    delete_ids = set(range(ctx.delete_count))
    collection.delete(ids=[str(i) for i in delete_ids])

    def reopen() -> BatchSearch:
        reopened_client = chromadb.PersistentClient(path=str(path))
        reopened_collection = reopened_client.get_collection("bench_vectors")

        def reopened_search(qs: np.ndarray, k: int) -> list[list[int]]:
            response = reopened_collection.query(query_embeddings=qs.tolist(), n_results=k, include=[])
            return [[int(x) for x in row] for row in response["ids"]]

        return reopened_search

    def concurrent_write() -> None:
        base = len(ctx.vectors)
        collection.add(
            ids=[str(base + i) for i in range(16)],
            embeddings=ctx.vectors[:16].tolist(),
            metadatas=[{"item_id": int(base + i), "unit_kind": "transcript"} for i in range(16)],
        )

    run_common_checks(result, search, ctx.queries, ctx.k, delete_ids, reopen, concurrent_write)
    result.disk_bytes = dir_size(path)
    return result


BACKENDS: dict[str, Callable[[BenchContext], BackendResult]] = {
    "zvec": bench_zvec,
    "lancedb": bench_lancedb,
    "lancedb_ivf_hnsw_pq": bench_lancedb_ivf_hnsw_pq,
    "sqlite_vec": bench_sqlite_vec,
    "usearch": bench_usearch,
    "usearch_high_recall": bench_usearch_high_recall,
    "usearch_exact": bench_usearch_exact,
    "turbovec": bench_turbovec,
    "qdrant_sidecar": bench_qdrant_sidecar,
    "chroma": bench_chroma,
}


def write_outputs(results: list[BackendResult], out_dir: Path, metadata: dict[str, Any]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    payload = {"metadata": metadata, "results": [result.to_dict() for result in results]}
    (out_dir / "results.json").write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")

    fields = list(BackendResult("x").to_dict().keys())
    with (out_dir / "results.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for result in results:
            writer.writerow(result.to_dict())


def main() -> int:
    args = parse_args()
    out_dir = Path(args.out_dir)
    if out_dir.exists():
        shutil.rmtree(out_dir)
    workdir = out_dir / "work"
    workdir.mkdir(parents=True, exist_ok=True)

    print(f"Generating dataset n={args.n} dim={args.dim} queries={args.queries} k={args.k}", flush=True)
    vectors, queries, ground_truth = generate_dataset(args.n, args.dim, args.queries, args.seed, args.k)

    qdrant_bin = Path(args.qdrant_bin).resolve()
    ctx = BenchContext(
        vectors=vectors,
        queries=queries,
        ground_truth=ground_truth,
        workdir=workdir,
        k=args.k,
        delete_count=min(100, args.n // 20),
        qdrant_bin=qdrant_bin,
    )

    metadata = {
        "python": sys.version,
        "platform": sys.platform,
        "n": args.n,
        "dim": args.dim,
        "queries": args.queries,
        "k": args.k,
        "seed": args.seed,
        "backends": args.backends,
        "qdrant_bin": str(qdrant_bin),
    }
    results: list[BackendResult] = []
    for backend in [name.strip() for name in args.backends.split(",") if name.strip()]:
        print(f"Running {backend}...", flush=True)
        runner = BACKENDS.get(backend)
        if runner is None:
            results.append(BackendResult(backend=backend, status="skipped", error="unknown backend"))
            continue
        try:
            started = time.perf_counter()
            result = runner(ctx)
            print(
                f"  {backend}: status={result.status} build={result.build_s} "
                f"avg_ms={result.query_avg_ms} recall={result.recall_at_k} "
                f"elapsed={time.perf_counter() - started:.2f}s",
                flush=True,
            )
            results.append(result)
        except Exception as exc:  # noqa: BLE001
            print(f"  {backend}: failed: {type(exc).__name__}: {exc}", flush=True)
            results.append(
                BackendResult(
                    backend=backend,
                    status="failed",
                    n=args.n,
                    dim=args.dim,
                    queries=args.queries,
                    error=f"{type(exc).__name__}: {exc}",
                )
            )
        gc.collect()
        write_outputs(results, out_dir, metadata)

    write_outputs(results, out_dir, metadata)
    print(f"Wrote {out_dir / 'results.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
