#!/usr/bin/env python3
"""Long-lived MLX runtime sidecar for Cerul local indexing.

The sidecar speaks newline-delimited JSON over stdin/stdout. Rust owns process
startup and request ordering; Python owns MLX model loading and inference.
Protocol responses always use stdout, while third-party library output is
redirected to stderr so it cannot corrupt the JSON stream.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import contextlib
import fnmatch
import gc
import hashlib
import importlib.metadata
import json
import math
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import threading
import time
import traceback
import unicodedata
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any
from urllib.parse import quote, urljoin


DEFAULT_EMBEDDING_MODEL = "mlx-community/Qwen3-VL-Embedding-2B-6bit"
DEFAULT_ASR_MODEL = "Qwen/Qwen3-ASR-0.6B"
DEFAULT_FORCED_ALIGNER_MODEL = "Qwen/Qwen3-ForcedAligner-0.6B"
DEFAULT_OCR_DET_MODEL = "PaddlePaddle/PP-OCRv6_small_det_onnx"
DEFAULT_OCR_REC_MODEL = "PaddlePaddle/PP-OCRv6_small_rec_onnx"
DEFAULT_WHISPER_MODEL = "mlx-community/whisper-large-v3-turbo"
DEFAULT_TEXT_EMBED_BATCH_SIZE = 8
DEFAULT_IMAGE_EMBED_BATCH_SIZE = 2
DEFAULT_MODEL_MIRROR_BASE_URL = "https://cdn.cerul.ai/models/v1"
DEFAULT_MODEL_MIRROR_USER_AGENT = "Cerul model-mirror"
DEFAULT_MODELSCOPE_ENDPOINT = "https://modelscope.cn"
PROBE_BYTES = 16 * 1024 * 1024
PROBE_WINDOW_SECS = 3.0
PROBE_TIMEOUT_SECS = 5.0

SOURCE_HUGGINGFACE = "huggingface"
SOURCE_MODELSCOPE = "modelscope"
SOURCE_CERUL_CDN = "cerul_cdn"
SOURCE_LABELS = {
    SOURCE_HUGGINGFACE: "Hugging Face",
    SOURCE_MODELSCOPE: "ModelScope",
    SOURCE_CERUL_CDN: "Cerul CDN",
}

QWEN3_VL_ALLOW_PATTERNS = [
    "*.json",
    "*.safetensors",
    "*.py",
    "*.tiktoken",
    "*.txt",
    "*.model",
    "*.jinja",
]
ONNX_OCR_ALLOW_PATTERNS = ["inference.onnx", "inference.yml", "README.md"]

ORIGINAL_STDOUT = sys.stdout
_STDOUT_LOCK = threading.Lock()
_PREPARE_STATUS_LOCK = threading.Lock()
MODELS_CACHE_ROOT: Path | None = None
_MIRROR_MANIFEST_LOADED = False
_MIRROR_MANIFEST_CACHE: dict[str, Any] | None = None

# Supply-chain pinning: default models always resolve to a reviewed revision
# instead of whatever the upstream repo's main branch points at today.
# Custom model ids supplied by the user are downloaded at their latest
# revision (documented behaviour).
PINNED_MODEL_REVISIONS = {
    DEFAULT_EMBEDDING_MODEL: "008fb7666d66aebeb3134aaec1d28f9806f81b6c",
    DEFAULT_ASR_MODEL: "5eb144179a02acc5e5ba31e748d22b0cf3e303b0",
    DEFAULT_FORCED_ALIGNER_MODEL: "c7cbfc2048c462b0d63a45797104fc9db3ad62b7",
    DEFAULT_OCR_DET_MODEL: "4fda2ea33fb340a1a19592aec4604ba1d2d5587d",
    DEFAULT_OCR_REC_MODEL: "2f0724790c8b57946c89cc45d2fa79e405781f51",
    DEFAULT_WHISPER_MODEL: "a4aaeec0636e6fef84abdcbe3544cb2bf7e9f6fb",
}
PINNED_SNAPSHOT_REQUIRED_FILES = {
    DEFAULT_EMBEDDING_MODEL: (
        "config.json",
        "preprocessor_config.json",
        "tokenizer.json",
        "tokenizer_config.json",
    ),
    DEFAULT_OCR_DET_MODEL: ("inference.onnx", "inference.yml"),
    DEFAULT_OCR_REC_MODEL: ("inference.onnx", "inference.yml"),
}
PINNED_SNAPSHOT_WEIGHT_GLOBS = ("*.safetensors", "*.bin", "*.npz", "*.onnx")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Cerul MLX JSONL sidecar")
    parser.add_argument("--models-cache", type=Path, required=True)
    parser.add_argument("--embedding-model", default=DEFAULT_EMBEDDING_MODEL)
    parser.add_argument("--asr-model", default=DEFAULT_ASR_MODEL)
    parser.add_argument("--forced-aligner-model", default=DEFAULT_FORCED_ALIGNER_MODEL)
    # In-memory quantization for the official ASR + forced-aligner weights.
    # "4bit" is the smallest (~-70% RAM, ~+0.43 WER); "8bit" is near-lossless
    # (~+0.04 WER); "none" keeps full fp16.
    parser.add_argument("--asr-quantization", default="4bit", choices=["8bit", "4bit", "none"])
    parser.add_argument("--ocr-det-model", default=DEFAULT_OCR_DET_MODEL)
    parser.add_argument("--ocr-rec-model", default=DEFAULT_OCR_REC_MODEL)
    parser.add_argument("--whisper-model", default=DEFAULT_WHISPER_MODEL)
    # One-shot model fetch: download the given HF repos into the cache and exit
    # (no JSONL loop, no model load). Used by the "prepare on-device models"
    # consent flow so the first index doesn't block on a silent download.
    parser.add_argument(
        "--prepare",
        nargs="*",
        default=None,
        metavar="REPO",
        help="Download the given model repos into the cache, then exit.",
    )
    return parser.parse_args()


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def configure_cache(models_cache: Path) -> None:
    global MODELS_CACHE_ROOT
    models_cache = models_cache.resolve()
    MODELS_CACHE_ROOT = models_cache
    hf_home = models_cache / "huggingface"
    hf_home.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("HF_HOME", str(hf_home))


def prepare_status_path() -> Path | None:
    if MODELS_CACHE_ROOT is None:
        return None
    return MODELS_CACHE_ROOT / "prepare-status.json"


def write_prepare_status(**fields: Any) -> None:
    path = prepare_status_path()
    if path is None:
        return
    payload = {
        "updated_at": time.time(),
        **fields,
    }
    with _PREPARE_STATUS_LOCK:
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_suffix(".json.tmp")
            tmp.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
            tmp.replace(path)
        except Exception as exc:  # noqa: BLE001 - status must never break downloads.
            print(f"prepare: failed to write status ({exc})", file=sys.stderr)


def normalize_download_source(value: str | None) -> str:
    source = (value or "auto").strip().lower().replace("-", "_")
    aliases = {
        "hf": SOURCE_HUGGINGFACE,
        "hugging_face": SOURCE_HUGGINGFACE,
        "huggingface": SOURCE_HUGGINGFACE,
        "model_scope": SOURCE_MODELSCOPE,
        "modelscope": SOURCE_MODELSCOPE,
        "cerul": SOURCE_CERUL_CDN,
        "cdn": SOURCE_CERUL_CDN,
        "cerulcdn": SOURCE_CERUL_CDN,
        "cerul_cdn": SOURCE_CERUL_CDN,
    }
    if source in {"", "auto"}:
        return "auto"
    return aliases.get(source, "auto")


def configured_download_source() -> str:
    return normalize_download_source(os.environ.get("CERUL_MODEL_DOWNLOAD_SOURCE"))


def default_source_order() -> list[str]:
    region = os.environ.get("CERUL_MODEL_DOWNLOAD_REGION", "").strip().lower()
    if region in {"cn", "china", "mainland", "zh-cn"}:
        return [SOURCE_MODELSCOPE, SOURCE_CERUL_CDN, SOURCE_HUGGINGFACE]
    return [SOURCE_HUGGINGFACE, SOURCE_CERUL_CDN, SOURCE_MODELSCOPE]


def source_label(source: str) -> str:
    return SOURCE_LABELS.get(source, source)


def env_positive_int(name: str, fallback: int) -> int:
    try:
        value = int(os.environ.get(name, ""))
    except ValueError:
        return fallback
    return max(1, value)


def env_truthy(name: str) -> bool:
    return os.environ.get(name, "").strip().lower() in {"1", "true", "yes", "on"}


def model_cache_dir_name(model_id_or_path: str) -> str:
    return f"models--{model_id_or_path.replace('/', '--')}"


def model_mirror_base_url() -> str | None:
    if env_truthy("CERUL_DISABLE_MODEL_MIRROR"):
        return None
    value = os.environ.get("CERUL_MODEL_MIRROR_BASE_URL", DEFAULT_MODEL_MIRROR_BASE_URL).strip()
    if value.lower() in {"", "0", "false", "off", "none"}:
        return None
    return value.rstrip("/")


def model_mirror_timeout() -> float:
    try:
        return max(1.0, float(os.environ.get("CERUL_MODEL_MIRROR_TIMEOUT_SECS", "30")))
    except ValueError:
        return 30.0


def model_mirror_user_agent() -> str:
    return os.getenv("CERUL_MODEL_MIRROR_USER_AGENT") or DEFAULT_MODEL_MIRROR_USER_AGENT


def model_mirror_request(url: str) -> urllib.request.Request:
    return urllib.request.Request(url, headers={"User-Agent": model_mirror_user_agent()})


def source_request(url: str, source: str) -> urllib.request.Request:
    if source == SOURCE_CERUL_CDN:
        return model_mirror_request(url)
    return urllib.request.Request(url, headers={"User-Agent": "Cerul model-downloader"})


def modelscope_endpoint() -> str:
    return os.environ.get("CERUL_MODELSCOPE_ENDPOINT", DEFAULT_MODELSCOPE_ENDPOINT).rstrip("/")


def hf_resolve_url(model_id_or_path: str, revision: str, file_path: str) -> str:
    return f"https://huggingface.co/{model_id_or_path}/resolve/{revision}/{quote(file_path)}"


def modelscope_resolve_url(model_id_or_path: str, file_path: str, revision: str = "master") -> str:
    return f"{modelscope_endpoint()}/models/{model_id_or_path}/resolve/{revision}/{quote(file_path)}"


def load_model_mirror_manifest() -> dict[str, Any] | None:
    global _MIRROR_MANIFEST_CACHE, _MIRROR_MANIFEST_LOADED
    if _MIRROR_MANIFEST_LOADED:
        return _MIRROR_MANIFEST_CACHE
    _MIRROR_MANIFEST_LOADED = True

    base_url = model_mirror_base_url()
    if not base_url:
        return None

    url = f"{base_url}/manifest.json"
    try:
        with urllib.request.urlopen(model_mirror_request(url), timeout=model_mirror_timeout()) as response:
            _MIRROR_MANIFEST_CACHE = json.loads(response.read().decode("utf-8"))
    except Exception as exc:  # noqa: BLE001 - mirror is optional.
        print(f"prepare: model mirror manifest unavailable ({exc}); falling back to Hugging Face", file=sys.stderr)
        _MIRROR_MANIFEST_CACHE = None
    return _MIRROR_MANIFEST_CACHE


def mirror_snapshot_dir(model_id_or_path: str, revision: str) -> Path | None:
    if MODELS_CACHE_ROOT is None:
        return None
    return (
        MODELS_CACHE_ROOT
        / "cerul-mirror"
        / model_cache_dir_name(model_id_or_path)
        / "snapshots"
        / revision
    )


def mirror_archive_paths(model_id_or_path: str, revision: str) -> tuple[Path, Path] | None:
    if MODELS_CACHE_ROOT is None:
        return None
    downloads = (
        MODELS_CACHE_ROOT
        / "cerul-mirror"
        / model_cache_dir_name(model_id_or_path)
        / "downloads"
    )
    archive = downloads / f"{revision}.tar.gz"
    partial = downloads / f"{revision}.tar.gz.partial"
    return archive, partial


def bundled_models_roots() -> list[Path]:
    roots: list[Path] = []
    env_root = os.environ.get("CERUL_BUNDLED_MODELS_DIR")
    if env_root:
        roots.append(Path(env_root))
    # Packaged: Resources/mlx-sidecar/cerul_mlx_sidecar.py -> Resources.
    # Dev: repo/mlx-sidecar/cerul_mlx_sidecar.py -> repo.
    roots.append(Path(__file__).resolve().parents[1] / "bundled-models")

    unique: list[Path] = []
    seen: set[str] = set()
    for root in roots:
        key = str(root.resolve()) if root.exists() else str(root)
        if key not in seen:
            seen.add(key)
            unique.append(root)
    return unique


def bundled_snapshot_dir(model_id_or_path: str, revision: str) -> Path | None:
    for root in bundled_models_roots():
        snapshot = root / model_cache_dir_name(model_id_or_path) / "snapshots" / revision
        missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
        if not missing_reasons:
            return snapshot
    return None


def mirror_entry(model_id_or_path: str, revision: str) -> dict[str, Any] | None:
    manifest = load_model_mirror_manifest()
    if not manifest:
        return None
    entry = (manifest.get("models") or {}).get(model_id_or_path)
    if not isinstance(entry, dict):
        return None
    if entry.get("revision") != revision:
        print(
            "prepare: model mirror revision mismatch for "
            f"{model_id_or_path} (wanted {revision}, got {entry.get('revision')}); falling back",
            file=sys.stderr,
        )
        return None
    archive = entry.get("archive") or {}
    if not archive.get("sha256"):
        return None
    base_url = model_mirror_base_url()
    archive_url = archive.get("url")
    if not archive_url and base_url and archive.get("path"):
        archive_url = urljoin(f"{base_url}/", str(archive["path"]))
    chunks = []
    for chunk in archive.get("chunks") or []:
        chunk_url = chunk.get("url")
        if not chunk_url and base_url and chunk.get("path"):
            chunk_url = urljoin(f"{base_url}/", str(chunk["path"]))
        if not chunk_url or not chunk.get("sha256"):
            return None
        chunks.append({**chunk, "url": chunk_url})
    if not archive_url and not chunks:
        archive = None
    files = []
    for file_entry in entry.get("files") or []:
        file_url = file_entry.get("url")
        if not file_url and base_url and file_entry.get("path"):
            file_url = urljoin(f"{base_url}/", str(file_entry["path"]))
        if not file_url or not file_entry.get("sha256") or not file_entry.get("snapshot_path"):
            return None
        files.append({**file_entry, "url": file_url})
    if archive is None and not files:
        return None
    return {
        **entry,
        "archive": {
            **archive,
            "url": archive_url,
            "chunks": chunks,
        } if archive else None,
        "files": files,
    }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_url_to_file(
    url: str,
    destination: Path,
    expected_sha256: str | None,
    *,
    source: str,
    model_id: str,
    file_label: str | None = None,
) -> None:
    expected_sha256 = (expected_sha256 or "").lower()
    if destination.is_file() and (not expected_sha256 or sha256_file(destination).lower() == expected_sha256):
        return

    destination.parent.mkdir(parents=True, exist_ok=True)
    partial_path = destination.with_name(f"{destination.name}.partial")
    resume_at = partial_path.stat().st_size if partial_path.exists() else 0
    request = source_request(url, source)
    if resume_at > 0:
        request.add_header("Range", f"bytes={resume_at}-")

    mode = "ab" if resume_at > 0 else "wb"
    started = time.monotonic()
    last_emit = started
    transferred = 0
    try:
        with urllib.request.urlopen(request, timeout=model_mirror_timeout()) as response:
            if resume_at > 0 and getattr(response, "status", None) != 206:
                mode = "wb"
            with partial_path.open(mode) as file:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    file.write(chunk)
                    transferred += len(chunk)
                    now = time.monotonic()
                    if now - last_emit >= 0.75:
                        elapsed = max(0.001, now - started)
                        write_prepare_status(
                            phase="downloading",
                            active_source=source,
                            source_label=source_label(source),
                            model_id=model_id,
                            file=file_label or destination.name,
                            download_bps=round(transferred / elapsed),
                        )
                        last_emit = now
    except urllib.error.HTTPError as exc:
        if exc.code == 416 and partial_path.exists():
            os.replace(partial_path, destination)
        else:
            raise
    else:
        os.replace(partial_path, destination)

    actual_sha256 = sha256_file(destination).lower()
    if expected_sha256 and actual_sha256 != expected_sha256:
        destination.unlink(missing_ok=True)
        raise RuntimeError(
            f"download checksum mismatch for {url}: expected {expected_sha256}, got {actual_sha256}"
        )


def download_mirror_archive(entry: dict[str, Any], archive_path: Path, partial_path: Path) -> None:
    archive = entry["archive"]
    expected_sha256 = str(archive["sha256"]).lower()
    if archive_path.is_file() and sha256_file(archive_path).lower() == expected_sha256:
        return

    chunks = archive.get("chunks") or []
    if chunks:
        chunks_dir = archive_path.parent / "chunks"
        archive_path.parent.mkdir(parents=True, exist_ok=True)
        partial_path.unlink(missing_ok=True)
        with partial_path.open("wb") as output:
            for chunk in chunks:
                chunk_name = Path(str(chunk["path"])).name
                chunk_path = chunks_dir / chunk_name
                download_url_to_file(
                    str(chunk["url"]),
                    chunk_path,
                    str(chunk["sha256"]),
                    source=SOURCE_CERUL_CDN,
                    model_id=str(entry.get("repo_id") or "model"),
                    file_label=chunk_name,
                )
                with chunk_path.open("rb") as input_file:
                    shutil.copyfileobj(input_file, output, length=1024 * 1024)
        os.replace(partial_path, archive_path)
        actual_sha256 = sha256_file(archive_path).lower()
        if actual_sha256 != expected_sha256:
            archive_path.unlink(missing_ok=True)
            raise RuntimeError(
                "model mirror archive checksum mismatch for "
                f"{entry.get('repo_id') or 'model'}: expected {expected_sha256}, got {actual_sha256}"
            )
        return

    if not archive.get("url"):
        raise RuntimeError("model mirror archive has neither url nor chunks")
    download_url_to_file(
        str(archive["url"]),
        archive_path,
        expected_sha256,
        source=SOURCE_CERUL_CDN,
        model_id=str(entry.get("repo_id") or "model"),
        file_label=archive_path.name,
    )


def download_mirror_files(entry: dict[str, Any], snapshot: Path) -> None:
    files = entry.get("files") or []
    if not files:
        raise RuntimeError("model mirror has no files manifest")
    snapshot.mkdir(parents=True, exist_ok=True)
    root = snapshot.resolve()
    for file_entry in files:
        relative = Path(str(file_entry["snapshot_path"]))
        destination = (snapshot / relative).resolve()
        if os.path.commonpath([str(root), str(destination)]) != str(root):
            raise RuntimeError(f"unsafe path in model mirror file manifest: {relative}")
        download_url_to_file(
            str(file_entry["url"]),
            destination,
            str(file_entry["sha256"]),
            source=SOURCE_CERUL_CDN,
            model_id=str(entry.get("repo_id") or "model"),
            file_label=str(relative),
        )


def safe_extract_tar_gz(archive_path: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temp_dir = Path(tempfile.mkdtemp(prefix=f".{destination.name}.", dir=destination.parent))
    temp_root = temp_dir.resolve()
    try:
        with tarfile.open(archive_path, "r:gz") as tar:
            for member in tar.getmembers():
                target = (temp_dir / member.name).resolve()
                if os.path.commonpath([str(temp_root), str(target)]) != str(temp_root):
                    raise RuntimeError(f"unsafe path in model mirror archive: {member.name}")
                if member.issym() or member.islnk():
                    raise RuntimeError(f"symlink not allowed in model mirror archive: {member.name}")
            tar.extractall(temp_dir)
        if destination.exists():
            shutil.rmtree(destination)
        temp_dir.rename(destination)
    except Exception:
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise


def resolve_mirror_snapshot(model_id_or_path: str, revision: str) -> Path | None:
    snapshot = mirror_snapshot_dir(model_id_or_path, revision)
    paths = mirror_archive_paths(model_id_or_path, revision)
    if snapshot is None or paths is None:
        return None
    missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
    if not missing_reasons:
        return snapshot

    entry = mirror_entry(model_id_or_path, revision)
    if entry is None:
        return None

    if entry.get("files"):
        print(f"prepare: downloading {model_id_or_path} files from Cerul mirror", file=sys.stderr)
        download_mirror_files(entry, snapshot)
        missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
        if missing_reasons:
            raise RuntimeError(
                "model mirror snapshot incomplete for "
                f"{model_id_or_path} ({'; '.join(missing_reasons)})"
            )
        return snapshot

    archive_path, partial_path = paths
    print(f"prepare: downloading {model_id_or_path} from Cerul mirror", file=sys.stderr)
    download_mirror_archive(entry, archive_path, partial_path)
    safe_extract_tar_gz(archive_path, snapshot)
    archive_path.unlink(missing_ok=True)

    missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
    if missing_reasons:
        raise RuntimeError(
            "model mirror snapshot incomplete for "
            f"{model_id_or_path} ({'; '.join(missing_reasons)})"
        )
    return snapshot


def modelscope_snapshot_dir(model_id_or_path: str) -> Path | None:
    if MODELS_CACHE_ROOT is None:
        return None
    return (
        MODELS_CACHE_ROOT
        / "modelscope"
        / model_cache_dir_name(model_id_or_path)
        / "snapshots"
        / "master"
    )


def modelscope_repo_files(model_id_or_path: str) -> list[dict[str, Any]]:
    url = (
        f"{modelscope_endpoint()}/api/v1/models/{model_id_or_path}"
        "/repo/files?revision=master&recursive=true"
    )
    with urllib.request.urlopen(source_request(url, SOURCE_MODELSCOPE), timeout=model_mirror_timeout()) as response:
        payload = json.loads(response.read().decode("utf-8"))
    if payload.get("Code") != 200:
        raise RuntimeError(f"ModelScope file API failed for {model_id_or_path}: {payload!r}")
    files = ((payload.get("Data") or {}).get("Files") or [])
    return [file for file in files if file.get("Type") == "blob" and file.get("Path")]


def file_allowed(path: str, allow_patterns: list[str] | None, model_id_or_path: str) -> bool:
    required = set(PINNED_SNAPSHOT_REQUIRED_FILES.get(model_id_or_path, ("config.json",)))
    if path in required:
        return True
    if allow_patterns is None:
        return True
    return any(fnmatch.fnmatch(path, pattern) for pattern in allow_patterns)


def resolve_modelscope_snapshot(model_id_or_path: str, allow_patterns: list[str] | None = None) -> Path | None:
    snapshot = modelscope_snapshot_dir(model_id_or_path)
    if snapshot is None:
        return None
    missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
    if not missing_reasons:
        return snapshot

    print(f"prepare: downloading {model_id_or_path} from ModelScope", file=sys.stderr)
    write_prepare_status(
        phase="downloading",
        active_source=SOURCE_MODELSCOPE,
        source_label=source_label(SOURCE_MODELSCOPE),
        model_id=model_id_or_path,
        download_bps=None,
    )
    files = [
        file
        for file in modelscope_repo_files(model_id_or_path)
        if file_allowed(str(file["Path"]), allow_patterns, model_id_or_path)
    ]
    if not files:
        raise RuntimeError(f"ModelScope repository has no usable files for {model_id_or_path}")

    snapshot.mkdir(parents=True, exist_ok=True)
    for file in files:
        remote_path = str(file["Path"])
        destination = snapshot / remote_path
        url = modelscope_resolve_url(model_id_or_path, remote_path)
        download_url_to_file(
            url,
            destination,
            str(file.get("Sha256") or ""),
            source=SOURCE_MODELSCOPE,
            model_id=model_id_or_path,
            file_label=remote_path,
        )

    missing_reasons = pinned_snapshot_missing_reasons(snapshot, model_id_or_path)
    if missing_reasons:
        raise RuntimeError(
            "ModelScope snapshot incomplete for "
            f"{model_id_or_path} ({'; '.join(missing_reasons)})"
        )
    return snapshot


def probe_url(source: str, url: str) -> dict[str, Any]:
    request = source_request(url, source)
    request.add_header("Range", f"bytes=0-{PROBE_BYTES - 1}")
    started = time.monotonic()
    bytes_read = 0
    try:
        with urllib.request.urlopen(request, timeout=PROBE_TIMEOUT_SECS) as response:
            first_byte_at: float | None = None
            while time.monotonic() - started < PROBE_WINDOW_SECS and bytes_read < PROBE_BYTES:
                chunk = response.read(256 * 1024)
                if not chunk:
                    break
                if first_byte_at is None:
                    first_byte_at = time.monotonic()
                bytes_read += len(chunk)
        elapsed = max(0.001, time.monotonic() - started)
        ttfb = (first_byte_at or time.monotonic()) - started
        return {
            "source": source,
            "ok": bytes_read > 0,
            "bytes_per_second": round(bytes_read / elapsed),
            "ttfb_ms": round(ttfb * 1000),
            "bytes": bytes_read,
        }
    except Exception as exc:  # noqa: BLE001 - probe failures only rank a source down.
        return {
            "source": source,
            "ok": False,
            "bytes_per_second": 0,
            "ttfb_ms": None,
            "bytes": bytes_read,
            "error": str(exc),
        }


def probe_url_for_source(source: str, model_id_or_path: str, revision: str) -> str | None:
    probe_file = "model.safetensors"
    if source == SOURCE_HUGGINGFACE:
        return hf_resolve_url(model_id_or_path, revision, probe_file)
    if source == SOURCE_MODELSCOPE:
        return modelscope_resolve_url(model_id_or_path, probe_file)
    if source == SOURCE_CERUL_CDN:
        entry = mirror_entry(model_id_or_path, revision)
        if entry is None:
            return None
        archive = entry.get("archive") or {}
        chunks = archive.get("chunks") or []
        if chunks:
            return str(chunks[0]["url"])
        if archive.get("url"):
            return str(archive["url"])
    return None


def select_download_source(model_id_or_path: str, revision: str) -> str:
    configured = configured_download_source()
    if configured != "auto":
        write_prepare_status(
            phase="downloading",
            active_source=configured,
            source_label=source_label(configured),
            model_id=model_id_or_path,
            download_bps=None,
        )
        return configured

    order = default_source_order()
    write_prepare_status(
        phase="probing",
        active_source=None,
        source_label=None,
        model_id=model_id_or_path,
        download_bps=None,
    )
    probe_inputs = []
    for source in order:
        url = probe_url_for_source(source, model_id_or_path, revision)
        if url:
            probe_inputs.append((source, url))

    results: list[dict[str, Any]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, len(probe_inputs))) as executor:
        futures = [executor.submit(probe_url, source, url) for source, url in probe_inputs]
        for future in concurrent.futures.as_completed(futures):
            results.append(future.result())

    ok_results = [result for result in results if result.get("ok")]
    if ok_results:
        # Prefer real throughput. For close ties, keep the default order stable.
        order_index = {source: index for index, source in enumerate(order)}
        ok_results.sort(
            key=lambda result: (
                int(result.get("bytes_per_second") or 0),
                -order_index.get(str(result.get("source")), 99),
            ),
            reverse=True,
        )
        selected = str(ok_results[0]["source"])
        write_prepare_status(
            phase="downloading",
            active_source=selected,
            source_label=source_label(selected),
            model_id=model_id_or_path,
            download_bps=int(ok_results[0].get("bytes_per_second") or 0),
            probes=results,
        )
        print(
            "prepare: selected "
            f"{source_label(selected)} for {model_id_or_path} "
            f"({int(ok_results[0].get('bytes_per_second') or 0)} B/s)",
            file=sys.stderr,
        )
        return selected

    selected = order[0]
    write_prepare_status(
        phase="downloading",
        active_source=selected,
        source_label=source_label(selected),
        model_id=model_id_or_path,
        download_bps=None,
        probes=results,
        last_source_error="all source probes failed; using default order",
    )
    return selected


def pinned_snapshot_missing_reasons(snapshot: Path, model_id_or_path: str) -> list[str]:
    if not snapshot.is_dir():
        return ["snapshot directory is missing"]
    try:
        has_entries = any(snapshot.iterdir())
    except OSError as exc:
        return [f"snapshot directory is unreadable: {exc}"]
    if not has_entries:
        return ["snapshot directory is empty"]

    required_files = PINNED_SNAPSHOT_REQUIRED_FILES.get(model_id_or_path, ("config.json",))
    missing = [name for name in required_files if not (snapshot / name).is_file()]
    if missing:
        return [f"missing {', '.join(missing)}"]

    if not any(any(snapshot.rglob(pattern)) for pattern in PINNED_SNAPSHOT_WEIGHT_GLOBS):
        return ["missing model weights"]
    return []


def resolve_snapshot(model_id_or_path: str, allow_patterns: list[str] | None = None) -> Path:
    local_path = Path(model_id_or_path)
    if local_path.exists():
        return local_path
    pinned = PINNED_MODEL_REVISIONS.get(model_id_or_path)
    if pinned:
        bundled = bundled_snapshot_dir(model_id_or_path, pinned)
        if bundled is not None:
            return bundled
    hf_home = os.environ.get("HF_HOME")
    if pinned and hf_home:
        cached = (
            Path(hf_home)
            / "hub"
            / f"models--{model_id_or_path.replace('/', '--')}"
            / "snapshots"
            / pinned
        )
        missing_reasons = pinned_snapshot_missing_reasons(cached, model_id_or_path)
        if not missing_reasons:
            return cached
        print(
            "prepare: pinned snapshot cache incomplete for "
            f"{model_id_or_path} ({'; '.join(missing_reasons)}); repairing",
            file=sys.stderr,
        )

    last_error: Exception | None = None
    if pinned:
        modelscope_cached = modelscope_snapshot_dir(model_id_or_path)
        if modelscope_cached is not None:
            missing_reasons = pinned_snapshot_missing_reasons(modelscope_cached, model_id_or_path)
            if not missing_reasons:
                return modelscope_cached

        selected_source = select_download_source(model_id_or_path, pinned)
        source_order = [selected_source] + [source for source in default_source_order() if source != selected_source]
        for source in source_order:
            try:
                if source == SOURCE_CERUL_CDN:
                    mirror_snapshot = resolve_mirror_snapshot(model_id_or_path, pinned)
                    if mirror_snapshot is not None:
                        return mirror_snapshot
                    raise RuntimeError("Cerul CDN manifest does not contain this model")
                if source == SOURCE_MODELSCOPE:
                    modelscope_snapshot = resolve_modelscope_snapshot(model_id_or_path, allow_patterns)
                    if modelscope_snapshot is not None:
                        return modelscope_snapshot
                    raise RuntimeError("ModelScope snapshot unavailable")
                if source == SOURCE_HUGGINGFACE:
                    break
            except Exception as exc:  # noqa: BLE001 - fallback to the next source.
                last_error = exc
                write_prepare_status(
                    phase="downloading",
                    active_source=None,
                    source_label=None,
                    model_id=model_id_or_path,
                    download_bps=None,
                    last_source_error=f"{source_label(source)} failed: {exc}",
                )
                print(
                    f"prepare: {source_label(source)} failed for {model_id_or_path} ({exc}); trying next source",
                    file=sys.stderr,
                )

    from huggingface_hub import snapshot_download

    write_prepare_status(
        phase="downloading",
        active_source=SOURCE_HUGGINGFACE,
        source_label=source_label(SOURCE_HUGGINGFACE),
        model_id=model_id_or_path,
        download_bps=None,
        last_source_error=str(last_error) if last_error else None,
    )
    return Path(
        snapshot_download(
            repo_id=model_id_or_path,
            revision=PINNED_MODEL_REVISIONS.get(model_id_or_path),
            allow_patterns=allow_patterns,
        )
    )


def allow_patterns_for_model(model_id_or_path: str) -> list[str] | None:
    if model_id_or_path == DEFAULT_EMBEDDING_MODEL:
        return QWEN3_VL_ALLOW_PATTERNS
    if model_id_or_path in {DEFAULT_OCR_DET_MODEL, DEFAULT_OCR_REC_MODEL}:
        return ONNX_OCR_ALLOW_PATTERNS
    return None


def patch_qwen3_vl_processor(processor: Any) -> list[str]:
    shims: list[str] = []
    inner = getattr(processor, "processor", processor)
    if not hasattr(inner, "image_ids"):
        inner.image_ids = [getattr(inner, "image_token_id", None)]
        shims.append("set Qwen3VLProcessor.image_ids")
    if not hasattr(inner, "video_ids"):
        inner.video_ids = [getattr(inner, "video_token_id", None)]
        shims.append("set Qwen3VLProcessor.video_ids")
    if not hasattr(inner, "audio_ids"):
        inner.audio_ids = [getattr(inner, "audio_token_id", None)]
        shims.append("set Qwen3VLProcessor.audio_ids")
    return shims


def patch_qwen3_vl_auto_image_processor() -> list[str]:
    """Keep mlx-embeddings' Qwen3-VL processor torch-free.

    mlx-embeddings 0.1.0 asks Transformers' AutoImageProcessor to build the
    Qwen3-VL image processor. In our packaged runtime, Transformers 5.x routes
    that through torch/torchvision-backed image processing even for text-only
    embedding loads. Cerul deliberately does not bundle torch, so install a
    narrow Qwen3-VL-only replacement before calling mlx_embeddings.load().
    """
    try:
        import numpy as np
        import mlx_embeddings.models.qwen3_vl.processor as qwen3_vl_processor
        from mlx_vlm.models.qwen3_vl.processing_qwen3_vl import (
            Qwen3VLImageProcessor,
            _qwen_vl_image_kwargs,
        )
    except Exception as exc:  # noqa: BLE001 - preserve the original load error.
        print(
            f"embedding: Qwen3-VL torch-free image patch unavailable ({exc})",
            file=sys.stderr,
        )
        return []

    current = getattr(qwen3_vl_processor, "AutoImageProcessor", None)
    if getattr(current, "_cerul_torch_free_qwen3_vl", False):
        return ["patch Qwen3-VL AutoImageProcessor to mlx-vlm torch-free processor"]

    def flatten_image_inputs(value: Any) -> list[Any]:
        if value is None:
            return []
        if isinstance(value, (str, os.PathLike)):
            return [str(value)]
        if isinstance(value, np.ndarray) or hasattr(value, "convert"):
            return [value]
        if isinstance(value, (list, tuple)):
            flattened: list[Any] = []
            for item in value:
                flattened.extend(flatten_image_inputs(item))
            return flattened
        return [value]

    class CerulQwen3VLImageProcessor(Qwen3VLImageProcessor):
        def fetch_images(self, images):
            return super().fetch_images(flatten_image_inputs(images))

        def __call__(self, images=None, **kwargs):
            overrides = {}
            for key in ("min_pixels", "max_pixels"):
                value = kwargs.get(key)
                if value is not None and hasattr(self, key):
                    overrides[key] = getattr(self, key)
                    setattr(self, key, value)
            try:
                return super().__call__(flatten_image_inputs(images), **kwargs)
            finally:
                for key, value in overrides.items():
                    setattr(self, key, value)

        def preprocess(self, images, **kwargs):
            return self(images, **kwargs)

    class CerulAutoImageProcessor:
        _cerul_torch_free_qwen3_vl = True

        @classmethod
        def from_pretrained(cls, pretrained_model_name_or_path, **kwargs):
            del cls
            image_kwargs = _qwen_vl_image_kwargs(
                pretrained_model_name_or_path,
                default_patch_size=16,
            )
            return CerulQwen3VLImageProcessor(**image_kwargs)

    qwen3_vl_processor.AutoImageProcessor = CerulAutoImageProcessor
    return ["patch Qwen3-VL AutoImageProcessor to mlx-vlm torch-free processor"]


class PaddleOnnxOcrRuntime:
    """PP-OCRv6 small ONNX detector + recognizer, CPU-only.

    The implementation keeps OCR independent from PaddleOCR/PaddleX so the
    packaged runtime only needs ONNX Runtime, OpenCV, PyYAML, and pyclipper.
    """

    def __init__(self, det_snapshot: Path, rec_snapshot: Path) -> None:
        import cv2
        import numpy as np
        import onnxruntime as ort
        import pyclipper
        import yaml

        self.cv2 = cv2
        self.np = np
        self.pyclipper = pyclipper

        det_config = yaml.safe_load((det_snapshot / "inference.yml").read_text(encoding="utf-8"))
        rec_config = yaml.safe_load((rec_snapshot / "inference.yml").read_text(encoding="utf-8"))
        det_post = det_config.get("PostProcess") or {}
        self.det_thresh = float(det_post.get("thresh", 0.2))
        self.det_box_thresh = float(det_post.get("box_thresh", 0.45))
        self.det_max_candidates = int(det_post.get("max_candidates", 3000))
        self.det_unclip_ratio = float(det_post.get("unclip_ratio", 1.4))
        self.det_size = 640
        self.rec_img_h = 48
        self.rec_img_w = 320
        self.rec_max_img_w = 3200
        self.rec_batch_size = 32
        character_dict = (rec_config.get("PostProcess") or {}).get("character_dict") or []
        self.rec_characters = [""] + [str(ch) for ch in character_dict] + [" "]

        options = ort.SessionOptions()
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        options.log_severity_level = 3
        providers = ["CPUExecutionProvider"]
        self.det_session = ort.InferenceSession(
            str(det_snapshot / "inference.onnx"),
            sess_options=options,
            providers=providers,
        )
        self.rec_session = ort.InferenceSession(
            str(rec_snapshot / "inference.onnx"),
            sess_options=options,
            providers=providers,
        )
        self.det_input_name = self.det_session.get_inputs()[0].name
        self.det_output_name = self.det_session.get_outputs()[0].name
        self.rec_input_name = self.rec_session.get_inputs()[0].name
        self.rec_output_name = self.rec_session.get_outputs()[0].name

    def preprocess_det(self, image: Any) -> tuple[Any, float, tuple[int, int]]:
        np = self.np
        cv2 = self.cv2
        h, w = image.shape[:2]
        scale = min(self.det_size / w, self.det_size / h)
        new_w = max(1, int(round(w * scale)))
        new_h = max(1, int(round(h * scale)))
        resized = cv2.resize(image, (new_w, new_h), interpolation=cv2.INTER_LINEAR)
        canvas = np.zeros((self.det_size, self.det_size, 3), dtype=np.uint8)
        canvas[:new_h, :new_w] = resized
        arr = canvas.astype("float32") / 255.0
        mean_values = np.array([0.485, 0.456, 0.406], dtype=np.float32)
        std_values = np.array([0.229, 0.224, 0.225], dtype=np.float32)
        arr = (arr - mean_values) / std_values
        return arr.transpose(2, 0, 1)[None, :, :, :].astype("float32"), scale, (new_h, new_w)

    def order_points_clockwise(self, points: Any) -> Any:
        np = self.np
        points = np.asarray(points, dtype=np.float32)
        x_sorted = points[np.argsort(points[:, 0]), :]
        left = x_sorted[:2, :]
        right = x_sorted[2:, :]
        left = left[np.argsort(left[:, 1]), :]
        right = right[np.argsort(right[:, 1]), :]
        return np.array([left[0], right[0], right[1], left[1]], dtype=np.float32)

    def mini_box(self, contour: Any) -> tuple[Any, float]:
        cv2 = self.cv2
        np = self.np
        rect = cv2.minAreaRect(contour)
        points = self.order_points_clockwise(cv2.boxPoints(rect))
        side_lengths = [
            np.linalg.norm(points[0] - points[1]),
            np.linalg.norm(points[1] - points[2]),
            np.linalg.norm(points[2] - points[3]),
            np.linalg.norm(points[3] - points[0]),
        ]
        return points, float(min(side_lengths))

    def box_score(self, pred: Any, box: Any) -> float:
        cv2 = self.cv2
        np = self.np
        h, w = pred.shape[:2]
        box = np.asarray(box, dtype=np.float32)
        xmin = max(0, int(np.floor(box[:, 0].min())))
        xmax = min(w - 1, int(np.ceil(box[:, 0].max())))
        ymin = max(0, int(np.floor(box[:, 1].min())))
        ymax = min(h - 1, int(np.ceil(box[:, 1].max())))
        if xmax < xmin or ymax < ymin:
            return 0.0
        mask = np.zeros((ymax - ymin + 1, xmax - xmin + 1), dtype=np.uint8)
        shifted = box.copy()
        shifted[:, 0] -= xmin
        shifted[:, 1] -= ymin
        cv2.fillPoly(mask, [shifted.astype(np.int32)], 1)
        values = pred[ymin : ymax + 1, xmin : xmax + 1][mask == 1]
        return float(values.mean()) if values.size else 0.0

    def unclip_box(self, box: Any) -> Any | None:
        cv2 = self.cv2
        np = self.np
        pyclipper = self.pyclipper
        area = abs(float(cv2.contourArea(box)))
        length = float(cv2.arcLength(box, True))
        if area <= 0 or length <= 0:
            return None
        distance = area * self.det_unclip_ratio / length
        offset = pyclipper.PyclipperOffset()
        offset.AddPath(box.astype(np.float32).tolist(), pyclipper.JT_ROUND, pyclipper.ET_CLOSEDPOLYGON)
        expanded = offset.Execute(distance)
        if not expanded:
            return None
        best = max(expanded, key=lambda path: abs(cv2.contourArea(np.asarray(path, dtype=np.float32))))
        return np.asarray(best, dtype=np.float32)

    def detect_boxes(self, image: Any) -> tuple[list[Any], list[float]]:
        np = self.np
        cv2 = self.cv2
        orig_h, orig_w = image.shape[:2]
        det_input, scale, resized_shape = self.preprocess_det(image)
        det_output = self.det_session.run([self.det_output_name], {self.det_input_name: det_input})[0]
        pred = np.asarray(det_output)
        if pred.ndim == 4:
            pred = pred[0, 0]
        elif pred.ndim == 3:
            pred = pred[0]
        bitmap = (pred > self.det_thresh).astype("uint8") * 255
        contours, _ = cv2.findContours(bitmap, cv2.RETR_LIST, cv2.CHAIN_APPROX_SIMPLE)
        contours = contours[: self.det_max_candidates]
        new_h, new_w = resized_shape
        candidates: list[tuple[Any, float]] = []
        for contour in contours:
            points, short_side = self.mini_box(contour)
            if short_side < 3:
                continue
            score = self.box_score(pred, points)
            if score < self.det_box_thresh:
                continue
            expanded = self.unclip_box(points)
            if expanded is None:
                continue
            points, short_side = self.mini_box(expanded.reshape(-1, 1, 2))
            if short_side < 5:
                continue
            if points[:, 0].max() > new_w + 3 or points[:, 1].max() > new_h + 3:
                continue
            points[:, 0] = np.clip(points[:, 0] / scale, 0, orig_w - 1)
            points[:, 1] = np.clip(points[:, 1] / scale, 0, orig_h - 1)
            candidates.append((self.order_points_clockwise(points), score))
        candidates.sort(key=lambda item: (float(item[0][:, 1].min()), float(item[0][:, 0].min())))
        return [box for box, _ in candidates], [float(score) for _, score in candidates]

    def crop_rotated(self, image: Any, points: Any) -> Any | None:
        np = self.np
        cv2 = self.cv2
        points = self.order_points_clockwise(points)
        width_a = np.linalg.norm(points[2] - points[3])
        width_b = np.linalg.norm(points[1] - points[0])
        height_a = np.linalg.norm(points[1] - points[2])
        height_b = np.linalg.norm(points[0] - points[3])
        width = int(max(width_a, width_b))
        height = int(max(height_a, height_b))
        if width < 2 or height < 2:
            return None
        dest = np.array([[0, 0], [width, 0], [width, height], [0, height]], dtype=np.float32)
        matrix = cv2.getPerspectiveTransform(points, dest)
        warped = cv2.warpPerspective(image, matrix, (width, height), borderMode=cv2.BORDER_REPLICATE)
        if warped.shape[0] / max(1, warped.shape[1]) >= 1.5:
            warped = np.rot90(warped)
        return warped

    def preprocess_rec(self, crops: list[Any]) -> Any:
        np = self.np
        cv2 = self.cv2
        max_wh_ratio = max(
            self.rec_img_w / self.rec_img_h,
            *(crop.shape[1] / max(1, crop.shape[0]) for crop in crops),
        )
        img_w = min(self.rec_max_img_w, int(math.ceil(self.rec_img_h * max_wh_ratio)))
        batch = np.zeros((len(crops), 3, self.rec_img_h, img_w), dtype=np.float32)
        for index, crop in enumerate(crops):
            h, w = crop.shape[:2]
            ratio = w / float(max(1, h))
            resized_w = min(img_w, max(1, int(math.ceil(self.rec_img_h * ratio))))
            resized = cv2.resize(crop, (resized_w, self.rec_img_h), interpolation=cv2.INTER_LINEAR)
            arr = resized.astype("float32").transpose(2, 0, 1) / 255.0
            arr -= 0.5
            arr /= 0.5
            batch[index, :, :, :resized_w] = arr
        return batch

    def decode_recognition(self, output: Any) -> tuple[list[str], list[float]]:
        np = self.np
        probs = np.asarray(output)
        indices = probs.argmax(axis=2)
        max_scores = probs.max(axis=2)
        texts: list[str] = []
        scores: list[float] = []
        for sequence, sequence_scores in zip(indices, max_scores):
            chars: list[str] = []
            char_scores: list[float] = []
            previous = None
            for index, score in zip(sequence.tolist(), sequence_scores.tolist()):
                if index == 0 or index == previous:
                    previous = index
                    continue
                if 0 <= index < len(self.rec_characters):
                    chars.append(self.rec_characters[index])
                    char_scores.append(float(score))
                previous = index
            texts.append("".join(chars).strip())
            scores.append(float(np.mean(char_scores)) if char_scores else 0.0)
        return texts, scores

    def recognize_crops(self, crops: list[Any]) -> tuple[list[str], list[float]]:
        if not crops:
            return [], []
        texts: list[str] = []
        scores: list[float] = []
        for start in range(0, len(crops), self.rec_batch_size):
            batch = crops[start : start + self.rec_batch_size]
            rec_input = self.preprocess_rec(batch)
            rec_output = self.rec_session.run([self.rec_output_name], {self.rec_input_name: rec_input})[0]
            batch_texts, batch_scores = self.decode_recognition(rec_output)
            texts.extend(batch_texts)
            scores.extend(batch_scores)
        return texts, scores

    def run(self, image_path: str) -> dict[str, Any]:
        image = self.cv2.imread(image_path, self.cv2.IMREAD_COLOR)
        if image is None:
            raise FileNotFoundError(image_path)
        boxes, box_scores = self.detect_boxes(image)
        crops: list[Any] = []
        kept_boxes: list[Any] = []
        kept_box_scores: list[float] = []
        for box, score in zip(boxes, box_scores):
            crop = self.crop_rotated(image, box)
            if crop is None:
                continue
            crops.append(crop)
            kept_boxes.append(box)
            kept_box_scores.append(score)
        texts, rec_scores = self.recognize_crops(crops)
        items = [
            {"text": text, "score": score, "box": box.astype("float32").tolist()}
            for text, score, box in zip(texts, rec_scores, kept_boxes)
            if text.strip()
        ]
        return {
            "text": "\n".join(item["text"] for item in items),
            "lines": items,
            "box_count": len(kept_boxes),
            "det_scores": kept_box_scores,
        }


class CerulMlxRuntime:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.embedding_model = None
        self.embedding_processor = None
        self.embedding_model_path: str | None = None
        self.embedding_shims: list[str] = []
        self.ocr_runtime: PaddleOnnxOcrRuntime | None = None
        self.ocr_det_model_path: str | None = None
        self.ocr_rec_model_path: str | None = None
        # Lazily-loaded, in-memory-quantized ASR + forced-aligner objects. Held
        # only while quantization is enabled (see _transcription_components).
        self._asr_model_obj = None
        self._asr_aligner_obj = None
        self.text_embed_batch_size = env_positive_int(
            "CERUL_MLX_TEXT_EMBED_BATCH_SIZE",
            DEFAULT_TEXT_EMBED_BATCH_SIZE,
        )
        self.image_embed_batch_size = env_positive_int(
            "CERUL_MLX_IMAGE_EMBED_BATCH_SIZE",
            DEFAULT_IMAGE_EMBED_BATCH_SIZE,
        )

    def _clear_accelerator_cache(self) -> None:
        with contextlib.suppress(Exception):
            import mlx.core as mx

            if hasattr(mx, "clear_cache"):
                mx.clear_cache()
        gc.collect()

    def release_embedding(self) -> None:
        self.embedding_model = None
        self.embedding_processor = None
        self.embedding_model_path = None
        self.embedding_shims = []
        self._clear_accelerator_cache()

    def release_ocr(self) -> None:
        self.ocr_runtime = None
        self.ocr_det_model_path = None
        self.ocr_rec_model_path = None
        self._clear_accelerator_cache()

    def release_transcription_runtime(self) -> None:
        # Per-transcription cleanup: free scratch buffers but KEEP the quantized
        # ASR + aligner objects warm, so indexing a queue of videos doesn't
        # reload and re-quantize them on every file. Only an explicit
        # release_models() drops the cached objects (see below).
        self._clear_accelerator_cache()

    def release_models(self, scope: str = "all") -> dict[str, Any]:
        normalized = scope.strip().lower()
        if normalized in {"transcription", "asr", "aligner", "all"}:
            self._asr_model_obj = None
            self._asr_aligner_obj = None
        if normalized in {"embedding", "all"}:
            self.release_embedding()
        if normalized in {"ocr", "all"}:
            self.release_ocr()
        if normalized in {"transcription", "asr", "aligner", "all"}:
            self.release_transcription_runtime()
        if normalized not in {"embedding", "ocr", "transcription", "asr", "aligner", "all"}:
            raise ValueError(f"unknown release scope: {scope}")
        return {"released": normalized, "loaded": self.loaded_state()}

    def loaded_state(self) -> dict[str, bool]:
        return {
            "embedding": self.embedding_model is not None,
            "ocr": self.ocr_runtime is not None,
            "asr": self._asr_model_obj is not None,
            "forced_aligner": self._asr_aligner_obj is not None,
        }

    def status(self) -> dict[str, Any]:
        apple_silicon = platform.system() == "Darwin" and platform.machine() == "arm64"
        packages = {
            "mlx": package_version("mlx"),
            "mlx-embeddings": package_version("mlx-embeddings"),
            "mlx-qwen3-asr": package_version("mlx-qwen3-asr"),
            "mlx-vlm": package_version("mlx-vlm"),
            "mlx-whisper": package_version("mlx-whisper"),
            "numpy": package_version("numpy"),
            "opencv-python": package_version("opencv-python"),
            "onnxruntime": package_version("onnxruntime"),
            "Pillow": package_version("Pillow"),
            "pyclipper": package_version("pyclipper"),
            "PyYAML": package_version("PyYAML"),
            "huggingface-hub": package_version("huggingface-hub"),
        }
        required = [
            "mlx",
            "mlx-embeddings",
            "mlx-qwen3-asr",
            "mlx-vlm",
            "opencv-python",
            "onnxruntime",
            "pyclipper",
            "PyYAML",
        ]
        missing = [name for name in required if packages.get(name) is None]
        return {
            "ok": apple_silicon and not missing,
            "platform": {
                "system": platform.system(),
                "machine": platform.machine(),
                "python": sys.version.split()[0],
            },
            "apple_silicon": apple_silicon,
            "packages": packages,
            "missing": missing,
            "models": {
                "embedding": self.args.embedding_model,
                "asr": self.args.asr_model,
                "asr_quantization": getattr(self.args, "asr_quantization", "none"),
                "forced_aligner": self.args.forced_aligner_model,
                "ocr_det": self.args.ocr_det_model,
                "ocr_rec": self.args.ocr_rec_model,
            },
            "cache": {"HF_HOME": os.environ.get("HF_HOME")},
            "loaded": self.loaded_state(),
        }

    def load_embedding(self) -> None:
        if self.embedding_model is not None:
            return
        from mlx_embeddings import load

        self.embedding_shims = patch_qwen3_vl_auto_image_processor()
        model_path = resolve_snapshot(self.args.embedding_model, QWEN3_VL_ALLOW_PATTERNS)
        self.embedding_model, self.embedding_processor = load(str(model_path))
        self.embedding_model_path = str(model_path)
        self.embedding_shims.extend(patch_qwen3_vl_processor(self.embedding_processor))

    def embed_texts(self, texts: list[str], instruction: str | None = None) -> dict[str, Any]:
        import mlx.core as mx
        import numpy as np

        self.load_embedding()
        payload: list[dict[str, str]] = []
        for text in texts:
            item = {"text": text}
            if instruction:
                item["instruction"] = instruction
            payload.append(item)

        arrays = []
        for start in range(0, len(payload), self.text_embed_batch_size):
            embeddings = self.embedding_model.process(
                payload[start : start + self.text_embed_batch_size],
                processor=self.embedding_processor,
            )
            mx.eval(embeddings)
            arrays.append(np.asarray(embeddings))
        array = np.concatenate(arrays, axis=0) if arrays else np.empty((0, 2048), dtype="float32")
        finite = bool(np.isfinite(array).all())
        if len(array.shape) != 2 or array.shape[1] != 2048:
            raise RuntimeError(f"embedding returned shape {list(array.shape)}, expected [N, 2048]")
        if not finite:
            raise RuntimeError("embedding returned NaN or Inf values")

        return {
            "vectors": array.astype("float32").tolist(),
            "shape": list(array.shape),
            "finite": finite,
            "dtype": str(array.dtype),
            "model_path": self.embedding_model_path,
            "compat_shims": self.embedding_shims,
        }

    def embed_images(self, paths: list[str]) -> dict[str, Any]:
        import mlx.core as mx
        import numpy as np

        self.load_embedding()
        payload = [{"image": path} for path in paths]
        arrays = []
        for start in range(0, len(payload), self.image_embed_batch_size):
            embeddings = self.embedding_model.process(
                payload[start : start + self.image_embed_batch_size],
                processor=self.embedding_processor,
            )
            mx.eval(embeddings)
            arrays.append(np.asarray(embeddings))
        array = np.concatenate(arrays, axis=0) if arrays else np.empty((0, 2048), dtype="float32")
        finite = bool(np.isfinite(array).all())
        if len(array.shape) != 2 or array.shape[1] != 2048:
            raise RuntimeError(f"image embedding returned shape {list(array.shape)}, expected [N, 2048]")
        if not finite:
            raise RuntimeError("image embedding returned NaN or Inf values")

        return {
            "vectors": array.astype("float32").tolist(),
            "shape": list(array.shape),
            "finite": finite,
            "dtype": str(array.dtype),
            "model_path": self.embedding_model_path,
            "compat_shims": self.embedding_shims,
        }

    def _asr_quant_bits(self) -> int | None:
        """Resolve the configured ASR/aligner quantization to a bit width.

        Returns None for full precision (fp16), keeping the no-quant path
        byte-for-byte identical to the previous behaviour.
        """
        value = (getattr(self.args, "asr_quantization", "none") or "none").strip().lower()
        return {"8bit": 8, "4bit": 4}.get(value)

    def _transcription_components(self, module: Any) -> tuple[Any, Any]:
        """Resolve the (model, forced_aligner) arguments for transcribe().

        Quantization off -> pass the HF repo ids; the library loads fp16 itself.
        Quantization on  -> load the *official* weights once, quantize them
        in-memory to N-bit, cache + reuse the objects, and hand those to
        transcribe(). Same official weights, just smaller/faster. The aligner's
        model lives on a lazily-built backend, so force it loaded before
        quantizing; if anything there fails we keep the aligner at fp16 rather
        than lose word-level timestamps.
        """
        bits = self._asr_quant_bits()
        if bits is None:
            return self.args.asr_model, self.args.forced_aligner_model

        import mlx.core as mx
        from mlx_qwen3_asr.convert import quantize_model

        if self._asr_model_obj is None:
            model, _config = module.load_model(self.args.asr_model, dtype=mx.float16)
            quantize_model(model, bits=bits, group_size=64)
            self._asr_model_obj = model
            print(f"asr: loaded {self.args.asr_model} ({bits}-bit)", file=sys.stderr)

        if self._asr_aligner_obj is None and self.args.forced_aligner_model:
            aligner = module.ForcedAligner(self.args.forced_aligner_model, dtype=mx.float16)
            try:
                aligner._ensure_loaded()
                quantize_model(aligner._backend.model, bits=bits, group_size=64)
                print(f"asr: loaded forced aligner ({bits}-bit)", file=sys.stderr)
            except Exception as exc:  # noqa: BLE001 - keep aligner at fp16 on failure
                print(f"asr: forced-aligner quantization skipped ({exc})", file=sys.stderr)
            self._asr_aligner_obj = aligner

        return self._asr_model_obj, (self._asr_aligner_obj or self.args.forced_aligner_model)

    def transcribe(self, audio_path: str, language: str | None = None) -> dict[str, Any]:
        # Accept both the bare name and full repo ids: a user setting
        # CERUL_MLX_ASR_MODEL to "mlx-community/whisper-large-v3-turbo" used
        # to fall through and be loaded as Qwen3-ASR weights, crashing with
        # no hint of the cause.
        asr_model_name = self.args.asr_model.rsplit("/", 1)[-1].lower()
        if asr_model_name.startswith("whisper"):
            return self.transcribe_with_mlx_whisper(audio_path, language)

        try:
            module = __import__("mlx_qwen3_asr")
            model_arg, aligner_arg = self._transcription_components(module)
            kwargs: dict[str, Any] = {
                "model": model_arg,
                "return_timestamps": True,
                "forced_aligner": aligner_arg,
            }
            if language and language != "auto":
                kwargs["language"] = language
            result = module.transcribe(audio_path, **kwargs)
            text = result.get("text") if isinstance(result, dict) else getattr(result, "text", "")
            raw_segments = result.get("segments") if isinstance(result, dict) else getattr(result, "segments", [])
            segments = [normalize_segment(segment) for segment in raw_segments or []]
            segments = [segment for segment in segments if segment["text"]]
            # The aligner returns one segment per spoken character; regroup into
            # readable phrase/sentence lines so the transcript isn't one glyph
            # per row.
            try:
                segments = group_aligned_segments(text or "", segments)
            except Exception as exc:  # noqa: BLE001 - keep raw segments on failure
                print(f"asr: line grouping skipped ({exc})", file=sys.stderr)
            return {
                "text": text or " ".join(segment["text"] for segment in segments),
                "segments": segments,
                "model": self.args.asr_model,
                "forced_aligner": self.args.forced_aligner_model,
                "quantization": getattr(self.args, "asr_quantization", "none"),
            }
        finally:
            self.release_transcription_runtime()

    def transcribe_with_mlx_whisper(self, audio_path: str, language: str | None = None) -> dict[str, Any]:
        try:
            import mlx_whisper

            # When --asr-model itself names a whisper repo, honour it instead
            # of silently substituting the default --whisper-model.
            whisper_model = (
                self.args.asr_model
                if "whisper" in self.args.asr_model.rsplit("/", 1)[-1].lower()
                and "/" in self.args.asr_model
                else self.args.whisper_model
            )
            kwargs: dict[str, Any] = {
                "path_or_hf_repo": whisper_model,
                "word_timestamps": True,
            }
            if language:
                kwargs["language"] = language
            output = mlx_whisper.transcribe(audio_path, **kwargs)
            raw_segments = output.get("segments") or []
            segments = [normalize_segment(segment) for segment in raw_segments]
            segments = [segment for segment in segments if segment["text"]]
            return {
                "text": output.get("text") or " ".join(segment["text"] for segment in segments),
                "segments": segments,
                "model": whisper_model,
            }
        finally:
            self.release_transcription_runtime()

    def load_ocr(self) -> None:
        if self.ocr_runtime is not None:
            return
        det_path = resolve_snapshot(self.args.ocr_det_model, allow_patterns_for_model(self.args.ocr_det_model))
        rec_path = resolve_snapshot(self.args.ocr_rec_model, allow_patterns_for_model(self.args.ocr_rec_model))
        self.ocr_runtime = PaddleOnnxOcrRuntime(det_path, rec_path)
        self.ocr_det_model_path = str(det_path)
        self.ocr_rec_model_path = str(rec_path)

    def ocr_images(self, paths: list[str], prompt: str | None = None) -> dict[str, Any]:
        del prompt
        self.load_ocr()
        if self.ocr_runtime is None:
            raise RuntimeError("OCR runtime failed to load")
        results = []
        for path in paths:
            result = self.ocr_runtime.run(path)
            results.append({"path": path, **result})
        return {
            "results": results,
            "model": {
                "det": self.args.ocr_det_model,
                "rec": self.args.ocr_rec_model,
                "det_path": self.ocr_det_model_path,
                "rec_path": self.ocr_rec_model_path,
            },
        }


def normalize_segment(segment: Any) -> dict[str, Any]:
    if isinstance(segment, dict):
        start = segment.get("start", segment.get("start_time", 0.0))
        end = segment.get("end", segment.get("end_time", start))
        text = segment.get("text", segment.get("word", ""))
    else:
        start = getattr(segment, "start", getattr(segment, "start_time", 0.0))
        end = getattr(segment, "end", getattr(segment, "end_time", start))
        text = getattr(segment, "text", getattr(segment, "word", ""))

    return {
        "start": float(start or 0.0),
        "end": float(end or start or 0.0),
        "text": str(text or "").strip(),
    }


# The Qwen3 ForcedAligner emits one segment per spoken token — one *character*
# for CJK, one *word* for spaced scripts — so a raw transcript renders one token
# per row. These knobs regroup tokens into readable subtitle-style lines.
# Targets are display COLUMNS (a CJK glyph counts as 2, everything else as 1) so
# one budget yields short CJK lines and sensibly word-wrapped Latin lines.
_LINE_HARD_BREAKS = set("。！？!?；;…\n")  # sentence enders — always end a line
_LINE_SOFT_BREAKS = set("，、：,:")  # clause punctuation — break once long enough
_LINE_SOFT_COLS = 12  # CJK ≈ 6 glyphs / Latin ≈ 12 cols before a comma may break
_LINE_MAX_COLS = 32  # CJK ≈ 16 glyphs / Latin ≈ 32 cols, wrapped at word bounds


def _is_punct_char(ch: str) -> bool:
    return ch.isspace() or unicodedata.category(ch).startswith("P")


def _is_cjk_char(ch: str) -> bool:
    code = ord(ch)
    return (
        0x4E00 <= code <= 0x9FFF  # CJK Unified Ideographs
        or 0x3400 <= code <= 0x4DBF  # CJK Extension A
        or 0x3040 <= code <= 0x30FF  # Hiragana + Katakana
        or 0xAC00 <= code <= 0xD7A3  # Hangul syllables
        or 0xF900 <= code <= 0xFAFF  # CJK Compatibility Ideographs
        or 0xFF00 <= code <= 0xFFEF  # Fullwidth / halfwidth forms
    )


def _col_width(text: str) -> int:
    return sum(2 if _is_cjk_char(ch) else 1 for ch in text)


def _segment_spoken_len(seg_text: str) -> int:
    spoken = sum(1 for ch in seg_text if not _is_punct_char(ch))
    return max(1, spoken)


def group_aligned_segments(
    text: str, segments: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Regroup per-token aligner segments into readable subtitle-style lines.

    `text` is the fully punctuated transcript; `segments` are spoken tokens (no
    punctuation), in order. We expand the segments to per-spoken-character
    timings, tokenise the text (CJK char / Latin word / punctuation / space),
    then pack tokens into lines — ending a line at sentence punctuation, at a
    comma once the line is long enough, or by wrapping at a word boundary near
    the column cap so a word is never split mid-way. Each line keeps the timing
    of its first/last spoken character. Works for Chinese and spaced scripts
    alike; falls back to the raw segments if anything looks off so we never lose
    the transcript.
    """
    if not segments or not text:
        return segments

    char_times: list[tuple[float, float]] = []
    for seg in segments:
        start = seg.get("start") or 0.0
        end = seg.get("end")
        end = end if end is not None else start
        for _ in range(_segment_spoken_len(seg.get("text") or "")):
            char_times.append((float(start), float(end)))
    if not char_times:
        return segments
    total = len(char_times)

    # Tokenise: CJK chars and Latin words are the atoms timings attach to;
    # punctuation and whitespace are separators that drive line breaks.
    atoms: list[tuple[str, int, str]] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch.isspace():
            atoms.append((ch, 0, "space"))
            i += 1
        elif _is_punct_char(ch):
            atoms.append((ch, 0, "punct"))
            i += 1
        elif _is_cjk_char(ch):
            atoms.append((ch, 1, "cjk"))
            i += 1
        else:
            j = i
            while (
                j < n
                and not text[j].isspace()
                and not _is_punct_char(text[j])
                and not _is_cjk_char(text[j])
            ):
                j += 1
            atoms.append((text[i:j], j - i, "word"))
            i = j

    lines: list[dict[str, Any]] = []
    buf: list[str] = []
    line_start: int | None = None
    idx = 0
    cols = 0
    pending_break = False

    def flush(end_idx: int) -> None:
        nonlocal buf, line_start, cols, pending_break
        line_text = "".join(buf).strip()
        if line_text and line_start is not None and line_start < total:
            last = min(end_idx, total) - 1
            start_sec = char_times[line_start][0]
            end_sec = char_times[last][1] if last >= line_start else char_times[line_start][1]
            lines.append({"start": start_sec, "end": end_sec, "text": line_text})
        buf = []
        line_start = None
        cols = 0
        pending_break = False

    for k, (atom, spoken, kind) in enumerate(atoms):
        if kind == "space":
            if buf:  # never lead a line with whitespace
                buf.append(atom)
            continue
        if kind == "punct":
            # An opening quote/bracket leads the next line, not the current one.
            if pending_break and unicodedata.category(atom) in ("Ps", "Pi"):
                flush(idx)
            buf.append(atom)
            nxt = atoms[k + 1] if k + 1 < len(atoms) else None
            # ASCII "." ends a sentence only when followed by space/end, so we
            # don't break decimals or initials.
            sentence_period = atom == "." and (nxt is None or nxt[2] == "space")
            if (
                atom in _LINE_HARD_BREAKS
                or sentence_period
                or (atom in _LINE_SOFT_BREAKS and cols >= _LINE_SOFT_COLS)
            ):
                pending_break = True
            continue
        # spoken atom (cjk glyph or whole word)
        if pending_break:
            flush(idx)
        width = _col_width(atom)
        if buf and cols + width > _LINE_MAX_COLS:
            flush(idx)  # wrap before this word — never split it
        if line_start is None:
            line_start = idx
        buf.append(atom)
        idx = min(idx + spoken, total)
        cols += width
    flush(idx)

    return lines or segments


def dispatch(runtime: CerulMlxRuntime, request: dict[str, Any]) -> Any:
    method = request.get("method")
    params = request.get("params") or {}

    if method == "health":
        return {"status": "ok"}
    if method == "status":
        return runtime.status()
    if method == "embed_texts":
        return runtime.embed_texts(
            list(params.get("texts") or []),
            params.get("instruction"),
        )
    if method == "embed_images":
        return runtime.embed_images(list(params.get("paths") or []))
    if method == "transcribe":
        return runtime.transcribe(str(params["audio_path"]), params.get("language"))
    if method == "ocr_images":
        return runtime.ocr_images(list(params.get("paths") or []), params.get("prompt"))
    if method == "release_models":
        return runtime.release_models(str(params.get("scope") or "all"))

    raise ValueError(f"unknown method: {method}")


def _write_message(message: dict[str, Any]) -> None:
    line = json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n"
    with _STDOUT_LOCK:
        ORIGINAL_STDOUT.write(line)
        ORIGINAL_STDOUT.flush()


def send_response(response: dict[str, Any]) -> None:
    _write_message(response)


def emit_progress(request_id: Any, **fields: Any) -> None:
    """Emit a heartbeat notification while a long request is still running.

    Rust treats any line as proof of life, so these stop the sidecar idle
    timeout from firing during a slow transcription, OCR, or embedding pass.
    """
    _write_message({"id": request_id, "event": "progress", **fields})


@contextlib.contextmanager
def heartbeat(request_id: Any, label: str, interval: float = 4.0):
    stop = threading.Event()
    started = time.monotonic()

    def _beat() -> None:
        while not stop.wait(interval):
            emit_progress(
                request_id,
                stage=label,
                elapsed_secs=round(time.monotonic() - started, 1),
            )

    thread = threading.Thread(target=_beat, name="mlx-heartbeat", daemon=True)
    thread.start()
    try:
        yield
    finally:
        stop.set()
        thread.join(timeout=1.0)


def main() -> int:
    global ORIGINAL_STDOUT

    # Take over file descriptor 1 entirely: keep a private dup for the JSONL
    # protocol and point fd 1 at stderr. redirect_stdout() only swaps the
    # Python-level sys.stdout object — MLX/Metal/tokenizers writing to fd 1
    # via printf used to corrupt the protocol stream.
    protocol_fd = os.dup(1)
    os.dup2(2, 1)
    ORIGINAL_STDOUT = os.fdopen(protocol_fd, "w", buffering=1)
    sys.stdout = sys.stderr

    args = parse_args()
    configure_cache(args.models_cache)

    # One-shot prepare: fetch the requested repos and exit before the JSONL loop.
    if args.prepare is not None:
        repos = list(dict.fromkeys(r for r in args.prepare if r))
        write_prepare_status(
            phase="probing" if repos else "ready",
            active_source=None,
            source_label=None,
            model_id=None,
            download_bps=None,
            total_repos=len(repos),
        )
        for index, repo in enumerate(repos, start=1):
            print(f"prepare: ({index}/{len(repos)}) downloading {repo}", file=sys.stderr)
            write_prepare_status(
                phase="probing",
                active_source=None,
                source_label=None,
                model_id=repo,
                download_bps=None,
                repo_index=index,
                total_repos=len(repos),
            )
            resolve_snapshot(repo, allow_patterns_for_model(repo))
            print(f"prepare: ({index}/{len(repos)}) ready {repo}", file=sys.stderr)
            write_prepare_status(
                phase="downloading",
                active_source=None,
                source_label=None,
                model_id=repo,
                download_bps=None,
                repo_index=index,
                total_repos=len(repos),
            )
        write_prepare_status(
            phase="ready",
            active_source=None,
            source_label=None,
            model_id=None,
            download_bps=None,
            total_repos=len(repos),
        )
        print(f"prepare: complete ({len(repos)} repos)", file=sys.stderr)
        return 0

    runtime = CerulMlxRuntime(args)

    for line in sys.stdin:
        if not line.strip():
            continue
        request_id = None
        try:
            request = json.loads(line)
            request_id = request.get("id")
            method = request.get("method") or "request"
            with contextlib.redirect_stdout(sys.stderr):
                with heartbeat(request_id, method):
                    result = dispatch(runtime, request)
            send_response({"id": request_id, "ok": True, "result": result})
        except Exception as exc:  # noqa: BLE001 - sidecar must report all failures over JSON.
            traceback.print_exc(file=sys.stderr)
            send_response(
                {
                    "id": request_id,
                    "ok": False,
                    "error": {
                        "type": type(exc).__name__,
                        "message": str(exc),
                    },
                }
            )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
