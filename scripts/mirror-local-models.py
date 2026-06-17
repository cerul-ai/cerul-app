#!/usr/bin/env python3
"""Build and publish Cerul local-model mirror archives.

The app consumes a small manifest from cdn.cerul.ai and downloads one
tar.gz snapshot per pinned model revision. Each model keeps its own archive
namespace; large archives are uploaded as per-model chunks because Wrangler's
remote R2 upload path has a 300 MiB object limit.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tarfile
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
SIDECAR_PATH = REPO_ROOT / "mlx-sidecar" / "cerul_mlx_sidecar.py"
DEFAULT_BUCKET = "cerul-cdn"
DEFAULT_PREFIX = "models/v1"
DEFAULT_BASE_URL = "https://cdn.cerul.ai/models/v1"
DEFAULT_CHUNK_SIZE_MIB = 256
MAX_WRANGLER_REMOTE_UPLOAD_MIB = 290


def load_sidecar_module() -> Any:
    spec = importlib.util.spec_from_file_location("cerul_mlx_sidecar", SIDECAR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {SIDECAR_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def cache_dir_name(repo_id: str) -> str:
    return f"models--{repo_id.replace('/', '--')}"


def snapshot_files(snapshot: Path) -> list[Path]:
    return sorted(path for path in snapshot.rglob("*") if path.is_file())


def create_archive(snapshot: Path, archive: Path) -> None:
    archive.parent.mkdir(parents=True, exist_ok=True)
    temp = archive.with_suffix(archive.suffix + ".tmp")
    temp.unlink(missing_ok=True)
    with tarfile.open(temp, "w:gz", dereference=True) as tar:
        for path in snapshot_files(snapshot):
            tar.add(path, arcname=path.relative_to(snapshot), recursive=False)
    temp.replace(archive)


def write_next_chunk(source: Any, chunk_path: Path, expected_size: int) -> str:
    digest = hashlib.sha256()
    temp = chunk_path.with_suffix(chunk_path.suffix + ".tmp")
    temp.unlink(missing_ok=True)
    remaining = expected_size
    with temp.open("wb") as output:
        while remaining > 0:
            block = source.read(min(4 * 1024 * 1024, remaining))
            if not block:
                raise EOFError(f"unexpected EOF while writing {chunk_path}")
            output.write(block)
            digest.update(block)
            remaining -= len(block)
    temp.replace(chunk_path)
    return digest.hexdigest()


def chunk_archive(
    archive: Path,
    archive_key: str,
    *,
    prefix: str,
    base_url: str,
    chunk_size_mib: int,
) -> list[dict[str, Any]]:
    chunk_size = chunk_size_mib * 1024 * 1024
    archive_size = archive.stat().st_size
    archive_dir_key = archive_key.rsplit("/", 1)[0]
    prefix_root = f"{prefix.rstrip('/')}/"
    chunks: list[dict[str, Any]] = []
    index = 0
    offset = 0

    with archive.open("rb") as source:
        while offset < archive_size:
            expected_size = min(chunk_size, archive_size - offset)
            chunk_name = f"{archive.name}.part-{index:03d}"
            chunk_path = archive.with_name(chunk_name)
            if chunk_path.exists() and chunk_path.stat().st_size == expected_size:
                digest = sha256_file(chunk_path)
                source.seek(expected_size, os.SEEK_CUR)
            else:
                digest = write_next_chunk(source, chunk_path, expected_size)

            chunk_key = f"{archive_dir_key}/{chunk_name}"
            rel_path = chunk_key.removeprefix(prefix_root)
            chunks.append(
                {
                    "index": index,
                    "path": rel_path,
                    "url": f"{base_url.rstrip('/')}/{rel_path}",
                    "sha256": digest,
                    "size": expected_size,
                }
            )
            offset += expected_size
            index += 1

    while True:
        stale = archive.with_name(f"{archive.name}.part-{index:03d}")
        if not stale.exists():
            break
        stale.unlink()
        index += 1

    return chunks


def run(cmd: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, env=env)


def upload(bucket: str, key: str, file: Path, content_type: str) -> None:
    run(
        [
            "wrangler",
            "r2",
            "object",
            "put",
            f"{bucket}/{key}",
            "--remote",
            "--file",
            str(file),
            "--content-type",
            content_type,
            "--cache-control",
            "public, max-age=31536000, immutable",
            "--force",
        ]
    )


def upload_manifest(bucket: str, key: str, file: Path) -> None:
    run(
        [
            "wrangler",
            "r2",
            "object",
            "put",
            f"{bucket}/{key}",
            "--remote",
            "--file",
            str(file),
            "--content-type",
            "application/json",
            "--cache-control",
            "public, max-age=300",
            "--force",
        ]
    )


def build_manifest(args: argparse.Namespace) -> dict[str, Any]:
    sidecar = load_sidecar_module()
    models = [
        sidecar.DEFAULT_EMBEDDING_MODEL,
        sidecar.DEFAULT_ASR_MODEL,
        sidecar.DEFAULT_FORCED_ALIGNER_MODEL,
        sidecar.DEFAULT_OCR_MODEL,
    ]
    if args.include_whisper:
        models.append(sidecar.DEFAULT_WHISPER_MODEL)

    licenses = {
        sidecar.DEFAULT_EMBEDDING_MODEL: "apache-2.0",
        sidecar.DEFAULT_ASR_MODEL: "apache-2.0",
        sidecar.DEFAULT_FORCED_ALIGNER_MODEL: "apache-2.0",
        sidecar.DEFAULT_OCR_MODEL: "apache-2.0",
        sidecar.DEFAULT_WHISPER_MODEL: "mit",
    }
    source_models = {
        sidecar.DEFAULT_EMBEDDING_MODEL: "Qwen/Qwen3-VL-Embedding-2B",
        sidecar.DEFAULT_OCR_MODEL: "Qwen/Qwen3-VL-2B-Instruct",
        sidecar.DEFAULT_WHISPER_MODEL: "openai/whisper-large-v3-turbo",
    }
    allow_patterns = {
        sidecar.DEFAULT_EMBEDDING_MODEL: sidecar.QWEN3_VL_ALLOW_PATTERNS,
        sidecar.DEFAULT_OCR_MODEL: sidecar.QWEN3_VL_ALLOW_PATTERNS,
    }

    env = os.environ.copy()
    env.setdefault("HF_HUB_DISABLE_XET", "1")
    if env["HF_HUB_DISABLE_XET"].strip().lower() in {"0", "false", "no", "off"}:
        env.setdefault("HF_XET_HIGH_PERFORMANCE", "1")
    os.environ["HF_HUB_DISABLE_XET"] = env["HF_HUB_DISABLE_XET"]
    if "HF_XET_HIGH_PERFORMANCE" in env:
        os.environ["HF_XET_HIGH_PERFORMANCE"] = env["HF_XET_HIGH_PERFORMANCE"]

    from huggingface_hub import snapshot_download

    manifest: dict[str, Any] = {
        "version": 1,
        "base_url": args.base_url.rstrip("/"),
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "models": {},
    }

    for repo_id in models:
        revision = sidecar.PINNED_MODEL_REVISIONS[repo_id]
        print(f"Resolving {repo_id}@{revision}", flush=True)
        snapshot = Path(
            snapshot_download(
                repo_id=repo_id,
                revision=revision,
                allow_patterns=allow_patterns.get(repo_id),
                cache_dir=args.hf_cache,
            )
        )
        archive_key = f"{args.prefix.rstrip('/')}/{cache_dir_name(repo_id)}/{revision}/snapshot.tar.gz"
        archive_path = args.out_dir / archive_key
        if archive_path.exists():
            print(f"Reusing {archive_path}", flush=True)
        else:
            create_archive(snapshot, archive_path)
        digest = sha256_file(archive_path)
        size = archive_path.stat().st_size
        chunks = chunk_archive(
            archive_path,
            archive_key,
            prefix=args.prefix,
            base_url=args.base_url,
            chunk_size_mib=args.chunk_size_mb,
        )
        rel_path = archive_key.removeprefix(f"{args.prefix.rstrip('/')}/")
        manifest["models"][repo_id] = {
            "repo_id": repo_id,
            "revision": revision,
            "source_url": f"https://huggingface.co/{repo_id}",
            "source_model": source_models.get(repo_id, repo_id),
            "license": licenses[repo_id],
            "archive": {
                "path": rel_path,
                "sha256": digest,
                "size": size,
                "content_type": "application/gzip",
                "chunk_size": args.chunk_size_mb * 1024 * 1024,
                "chunks": chunks,
            },
        }
        print(
            f"Built {archive_path} ({size:,} bytes sha256={digest}, chunks={len(chunks)})",
            flush=True,
        )
        if args.upload:
            for chunk in chunks:
                chunk_key = f"{args.prefix.rstrip('/')}/{chunk['path']}"
                chunk_path = args.out_dir / chunk_key
                upload(args.bucket, chunk_key, chunk_path, "application/octet-stream")

    return manifest


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build Cerul local-model R2 mirror artifacts.")
    parser.add_argument("--bucket", default=DEFAULT_BUCKET)
    parser.add_argument("--prefix", default=DEFAULT_PREFIX)
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument("--out-dir", type=Path, default=REPO_ROOT / "target" / "model-mirror")
    parser.add_argument("--hf-cache", type=Path, default=None)
    parser.add_argument("--chunk-size-mb", type=int, default=DEFAULT_CHUNK_SIZE_MIB)
    parser.add_argument("--upload", action="store_true")
    parser.add_argument("--include-whisper", action="store_true")
    args = parser.parse_args()
    if args.chunk_size_mb < 1 or args.chunk_size_mb > MAX_WRANGLER_REMOTE_UPLOAD_MIB:
        parser.error(f"--chunk-size-mb must be between 1 and {MAX_WRANGLER_REMOTE_UPLOAD_MIB}")
    return args


def main() -> int:
    args = parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    manifest = build_manifest(args)
    manifest_key = f"{args.prefix.rstrip('/')}/manifest.json"
    manifest_path = args.out_dir / manifest_key
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    print(f"Wrote {manifest_path}", flush=True)
    if args.upload:
        upload_manifest(args.bucket, manifest_key, manifest_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
