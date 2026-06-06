#!/usr/bin/env python3
"""Validate the MLX model path selected for Cerul P0.

The script is intentionally a spike harness, not product runtime code. It
records whether the selected third-party packages satisfy the exact constraints
needed before Cerul starts integrating a long-lived MLX sidecar.
"""

from __future__ import annotations

import argparse
import importlib.metadata
import json
import os
import platform
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


DEFAULT_EMBEDDING_MODEL = "mlx-community/Qwen3-VL-Embedding-2B-6bit"
DEFAULT_ASR_MODEL = "mlx-community/Qwen3-ASR-0.6B-bf16"
EMBEDDING_ALLOW_PATTERNS = [
    "*.json",
    "*.safetensors",
    "*.py",
    "*.tiktoken",
    "*.txt",
    "*.model",
    "*.jinja",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Cerul's P0 MLX embedding and ASR feasibility probes."
    )
    parser.add_argument("--models-cache", type=Path, help="Cache directory for HF downloads.")
    parser.add_argument("--audio", type=Path, help="16 kHz mono speech WAV for ASR probing.")
    parser.add_argument("--report", type=Path, help="Write the JSON report to this path.")
    parser.add_argument("--embedding-model", default=DEFAULT_EMBEDDING_MODEL)
    parser.add_argument("--asr-model", default=DEFAULT_ASR_MODEL)
    parser.add_argument("--language", default="en", help="Language hint passed to ASR.")
    parser.add_argument("--skip-embedding", action="store_true")
    parser.add_argument("--skip-asr", action="store_true")
    parser.add_argument(
        "--check-prereqs",
        action="store_true",
        help="Only check local prerequisites; do not download or load models.",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit non-zero when any selected P0 requirement is not satisfied.",
    )
    return parser.parse_args()


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def configure_cache(models_cache: Path | None) -> dict[str, str]:
    if models_cache is None:
        return {}

    models_cache = models_cache.resolve()
    models_cache.mkdir(parents=True, exist_ok=True)
    hf_home = models_cache / "huggingface"
    hf_home.mkdir(parents=True, exist_ok=True)
    os.environ["HF_HOME"] = str(hf_home)
    return {"HF_HOME": str(hf_home)}


def base_report(args: argparse.Namespace, cache_env: dict[str, str]) -> dict[str, Any]:
    packages = {
        "mlx": package_version("mlx"),
        "mlx-embeddings": package_version("mlx-embeddings"),
        "qwen3-asr-mlx": package_version("qwen3-asr-mlx"),
        "numpy": package_version("numpy"),
        "pillow": package_version("Pillow"),
        "soundfile": package_version("soundfile"),
        "socksio": package_version("socksio"),
        "torch": package_version("torch"),
        "torchvision": package_version("torchvision"),
    }
    return {
        "p0": "mlx-runtime-and-indexing",
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": {
            "system": platform.system(),
            "machine": platform.machine(),
            "mac_ver": platform.mac_ver()[0],
            "python": sys.version.split()[0],
        },
        "models": {
            "embedding": args.embedding_model,
            "asr": args.asr_model,
        },
        "cache": cache_env,
        "packages": packages,
        "probes": {},
        "conclusion": {},
    }


def prereq_status(args: argparse.Namespace, report: dict[str, Any]) -> bool:
    failures: list[str] = []
    platform_info = report["platform"]
    packages = report["packages"]

    if platform_info["system"] != "Darwin" or platform_info["machine"] != "arm64":
        failures.append("MLX requires Apple Silicon macOS for this spike.")

    for name, version in packages.items():
        if version is None:
            failures.append(f"missing Python package: {name}")

    if args.audio is not None and not args.audio.is_file():
        failures.append(f"audio file not found: {args.audio}")

    if args.models_cache is not None and not os.access(args.models_cache, os.W_OK):
        failures.append(f"models cache is not writable: {args.models_cache}")

    report["probes"]["prereqs"] = {
        "ok": not failures,
        "failures": failures,
    }
    return not failures


def make_sample_image(path: Path) -> None:
    from PIL import Image, ImageDraw

    image = Image.new("RGB", (96, 96), color=(18, 31, 42))
    draw = ImageDraw.Draw(image)
    draw.rectangle((12, 16, 84, 80), outline=(88, 180, 156), width=4)
    draw.line((20, 70, 48, 42, 76, 68), fill=(232, 180, 90), width=5)
    draw.ellipse((40, 24, 56, 40), fill=(238, 238, 232))
    image.save(path)


def resolve_embedding_model(model_id_or_path: str) -> Path:
    local_path = Path(model_id_or_path)
    if local_path.exists():
        return local_path

    from huggingface_hub import snapshot_download

    return Path(
        snapshot_download(
            repo_id=model_id_or_path,
            allow_patterns=EMBEDDING_ALLOW_PATTERNS,
        )
    )


def run_embedding_probe(model_id: str) -> dict[str, Any]:
    import mlx.core as mx
    import numpy as np
    from mlx_embeddings import load

    started = time.time()
    model_path = resolve_embedding_model(model_id)
    model, processor = load(str(model_path))
    compat_shims: list[str] = []
    inner_processor = getattr(processor, "processor", processor)
    if not hasattr(inner_processor, "image_ids"):
        inner_processor.image_ids = [getattr(inner_processor, "image_token_id", None)]
        compat_shims.append("set Qwen3VLProcessor.image_ids")
    if not hasattr(inner_processor, "video_ids"):
        inner_processor.video_ids = [getattr(inner_processor, "video_token_id", None)]
        compat_shims.append("set Qwen3VLProcessor.video_ids")
    if not hasattr(inner_processor, "audio_ids"):
        inner_processor.audio_ids = [getattr(inner_processor, "audio_token_id", None)]
        compat_shims.append("set Qwen3VLProcessor.audio_ids")

    with tempfile.TemporaryDirectory(prefix="cerul-mlx-p0-") as tmp:
        image_path = Path(tmp) / "sample.png"
        make_sample_image(image_path)
        inputs = [
            {
                "text": "Find clips where someone explains model runtime progress.",
                "instruction": "Retrieve images or text relevant to the user's query.",
            },
            {"text": "A dashboard shows indexing progress for a local video search app."},
            {"image": str(image_path)},
        ]
        embeddings = model.process(inputs, processor=processor)
        mx.eval(embeddings)
        array = np.asarray(embeddings)

    shape = list(array.shape)
    finite = bool(np.isfinite(array).all())
    return {
        "ok": len(shape) == 2 and shape[0] == 3 and shape[1] == 2048 and finite,
        "shape": shape,
        "finite": finite,
        "dtype": str(array.dtype),
        "model_path": str(model_path),
        "compat_shims": compat_shims,
        "elapsed_seconds": round(time.time() - started, 3),
    }


def segment_has_timestamps(segment: Any) -> bool:
    if isinstance(segment, dict):
        return all(key in segment for key in ("start", "end", "text"))
    return all(hasattr(segment, key) for key in ("start", "end", "text"))


def run_asr_probe(model_id: str, audio: Path, language: str) -> dict[str, Any]:
    from qwen3_asr_mlx import Qwen3ASR

    started = time.time()
    with Qwen3ASR.from_pretrained(model_id) as model:
        result = model.transcribe(audio, language=language)

    result_fields = {
        "text": getattr(result, "text", None),
        "language": getattr(result, "language", None),
        "duration": getattr(result, "duration", None),
    }
    segments = getattr(result, "segments", None)
    has_segments = isinstance(segments, list) and len(segments) > 0
    has_timestamped_segments = has_segments and all(segment_has_timestamps(s) for s in segments)

    return {
        "ok": has_timestamped_segments,
        "transcribed": bool(result_fields["text"]),
        "result_type": type(result).__name__,
        "result_fields": {
            "text_length": len(result_fields["text"] or ""),
            "language": result_fields["language"],
            "duration": result_fields["duration"],
        },
        "has_segments": has_segments,
        "has_timestamped_segments": bool(has_timestamped_segments),
        "public_attributes": sorted(
            name for name in dir(result) if not name.startswith("_")
        ),
        "elapsed_seconds": round(time.time() - started, 3),
    }


def capture_probe(name: str, probe_fn, *args: Any) -> dict[str, Any]:
    try:
        return probe_fn(*args)
    except Exception as exc:  # noqa: BLE001 - spike report must preserve failures.
        return {
            "ok": False,
            "error_type": type(exc).__name__,
            "error": str(exc),
            "probe": name,
        }


def write_report(report: dict[str, Any], path: Path | None) -> None:
    text = json.dumps(report, indent=2, sort_keys=True)
    if path is not None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text + "\n", encoding="utf-8")
    print(text)


def main() -> int:
    args = parse_args()
    cache_env = configure_cache(args.models_cache)
    report = base_report(args, cache_env)

    prereqs_ok = prereq_status(args, report)
    if args.check_prereqs:
        report["conclusion"] = {
            "ok": prereqs_ok,
            "embedding_2048": None,
            "asr_timestamped_segments": None,
        }
        write_report(report, args.report)
        return 0 if prereqs_ok else 2

    if not prereqs_ok:
        report["conclusion"] = {
            "ok": False,
            "embedding_2048": None,
            "asr_timestamped_segments": None,
        }
        write_report(report, args.report)
        return 2

    if args.skip_embedding:
        report["probes"]["embedding"] = {"skipped": True}
    else:
        report["probes"]["embedding"] = capture_probe(
            "embedding", run_embedding_probe, args.embedding_model
        )

    if args.skip_asr:
        report["probes"]["asr"] = {"skipped": True}
    elif args.audio is None:
        report["probes"]["asr"] = {
            "skipped": True,
            "reason": "pass --audio with a speech WAV to run the ASR probe",
        }
    else:
        report["probes"]["asr"] = capture_probe(
            "asr", run_asr_probe, args.asr_model, args.audio, args.language
        )

    embedding_ok = report["probes"].get("embedding", {}).get("ok")
    asr_ok = report["probes"].get("asr", {}).get("ok")
    selected_results = [
        value
        for value in (embedding_ok, asr_ok)
        if value is not None
    ]
    conclusion_ok = all(selected_results) if selected_results else False
    report["conclusion"] = {
        "ok": conclusion_ok,
        "embedding_2048": embedding_ok,
        "asr_timestamped_segments": asr_ok,
    }
    write_report(report, args.report)

    if args.strict and not conclusion_ok:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
