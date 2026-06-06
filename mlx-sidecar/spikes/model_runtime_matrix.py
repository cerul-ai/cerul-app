#!/usr/bin/env python3
"""Probe Cerul's candidate product model runtimes.

This is a feasibility matrix, not production runtime code. It answers two
questions before we wire models into the app:

1. Can the model/runtime run on the Apple Silicon local target?
2. Does the output satisfy Cerul's product contract without CPU model inference?
"""

from __future__ import annotations

import argparse
import importlib
import importlib.metadata
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Callable


DEFAULT_EMBEDDING_MODEL = "mlx-community/Qwen3-VL-Embedding-2B-6bit"
DEFAULT_RERANKER_MODEL = "Qwen/Qwen3-VL-Reranker-2B"
DEFAULT_TEXT_RERANKER_MODEL = "mlx-community/Qwen3-Reranker-0.6B-mxfp8"
DEFAULT_QWEN_ASR_MODEL = "Qwen/Qwen3-ASR-0.6B"
DEFAULT_MLX_WHISPER_MODEL = "mlx-community/whisper-large-v3-turbo"
DEFAULT_FORCED_ALIGNER_MODEL = "Qwen/Qwen3-ForcedAligner-0.6B"
DEFAULT_OCR_MODEL = "mlx-community/Qwen3-VL-2B-Instruct-4bit"
DEFAULT_VAD_MODEL = "onnx-community/silero-vad"
DEFAULT_VAD_FILE = "onnx/model_quantized.onnx"
DEFAULT_ONNX_OCR_MODEL = "bukuroo/PPOCRv5-ONNX"

ALL_PROBES = [
    "mlx-embedding",
    "mlx-reranker",
    "mlx-text-reranker",
    "mlx-qwen3-asr",
    "mlx-whisper",
    "whispermlx",
    "onnx-vad",
    "mlx-ocr",
    "onnx-ocr",
    "forced-aligner",
    "server-infra",
]

QWEN3_VL_ALLOW_PATTERNS = [
    "*.json",
    "*.safetensors",
    "*.py",
    "*.tiktoken",
    "*.txt",
    "*.model",
    "*.jinja",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Cerul model runtime matrix probes.")
    parser.add_argument("--models-cache", type=Path, help="Shared model cache directory.")
    parser.add_argument("--audio", type=Path, help="16 kHz mono speech WAV for ASR probes.")
    parser.add_argument("--report", type=Path, help="Write JSON report to this path.")
    parser.add_argument(
        "--markdown-report",
        type=Path,
        help="Write a concise Markdown report to this path.",
    )
    parser.add_argument(
        "--probe",
        action="append",
        choices=ALL_PROBES,
        help="Probe to run. Omit to run all local-safe probes.",
    )
    parser.add_argument("--embedding-model", default=DEFAULT_EMBEDDING_MODEL)
    parser.add_argument("--reranker-model", default=DEFAULT_RERANKER_MODEL)
    parser.add_argument("--text-reranker-model", default=DEFAULT_TEXT_RERANKER_MODEL)
    parser.add_argument("--qwen-asr-model", default=DEFAULT_QWEN_ASR_MODEL)
    parser.add_argument("--mlx-whisper-model", default=DEFAULT_MLX_WHISPER_MODEL)
    parser.add_argument("--forced-aligner-model", default=DEFAULT_FORCED_ALIGNER_MODEL)
    parser.add_argument("--ocr-model", default=DEFAULT_OCR_MODEL)
    parser.add_argument("--vad-model", default=DEFAULT_VAD_MODEL)
    parser.add_argument("--vad-onnx-file", default=DEFAULT_VAD_FILE)
    parser.add_argument("--onnx-ocr-model", default=DEFAULT_ONNX_OCR_MODEL)
    parser.add_argument(
        "--check-prereqs",
        action="store_true",
        help="Check local environment and package imports only.",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit non-zero if a selected probe does not meet product criteria.",
    )
    parser.add_argument(
        "--no-isolate",
        action="store_true",
        help="Run multiple probes in the same Python process. Default multi-probe runs isolate each probe.",
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
    return {
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": {
            "system": platform.system(),
            "machine": platform.machine(),
            "mac_ver": platform.mac_ver()[0],
            "python": sys.version.split()[0],
        },
        "models": {
            "embedding": args.embedding_model,
            "reranker": args.reranker_model,
            "text_reranker": args.text_reranker_model,
            "qwen_asr": args.qwen_asr_model,
            "mlx_whisper": args.mlx_whisper_model,
            "forced_aligner": args.forced_aligner_model,
            "ocr": args.ocr_model,
            "vad": args.vad_model,
            "onnx_ocr": args.onnx_ocr_model,
        },
        "cache": cache_env,
        "packages": {
            "mlx": package_version("mlx"),
            "mlx-embeddings": package_version("mlx-embeddings"),
            "mlx-whisper": package_version("mlx-whisper"),
            "mlx-qwen3-asr": package_version("mlx-qwen3-asr"),
            "mlx-lm": package_version("mlx-lm"),
            "mlx-vlm": package_version("mlx-vlm"),
            "whispermlx": package_version("whispermlx"),
            "onnxruntime": package_version("onnxruntime"),
            "numpy": package_version("numpy"),
            "pillow": package_version("Pillow"),
            "soundfile": package_version("soundfile"),
            "torch": package_version("torch"),
            "torchvision": package_version("torchvision"),
        },
        "probes": {},
        "summary": {},
    }


def check_prereqs(args: argparse.Namespace, report: dict[str, Any]) -> bool:
    failures: list[str] = []
    platform_info = report["platform"]
    package_names = [
        "mlx",
        "mlx-embeddings",
        "mlx-lm",
        "mlx-vlm",
        "mlx-whisper",
        "mlx-qwen3-asr",
        "onnxruntime",
        "numpy",
        "pillow",
        "soundfile",
        "torch",
        "torchvision",
    ]

    if platform_info["system"] != "Darwin" or platform_info["machine"] != "arm64":
        failures.append("Cerul v1 local model runtime target is Apple Silicon macOS.")

    for name in package_names:
        if report["packages"].get(name) is None:
            failures.append(f"missing Python package: {name}")

    if args.audio is not None and not args.audio.is_file():
        failures.append(f"audio file not found: {args.audio}")

    if args.models_cache is not None and not os.access(args.models_cache, os.W_OK):
        failures.append(f"models cache is not writable: {args.models_cache}")

    report["probes"]["prereqs"] = {"ok": not failures, "failures": failures}
    return not failures


def make_sample_image(path: Path) -> None:
    from PIL import Image, ImageDraw, ImageFont

    image = Image.new("RGB", (512, 256), color="white")
    draw = ImageDraw.Draw(image)
    try:
        font = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 72)
    except OSError:
        font = ImageFont.load_default()
    draw.text((48, 82), "CERUL", fill="black", font=font)
    image.save(path)


def resolve_snapshot(model_id_or_path: str, allow_patterns: list[str] | None = None) -> Path:
    local_path = Path(model_id_or_path)
    if local_path.exists():
        return local_path

    from huggingface_hub import snapshot_download

    return Path(snapshot_download(repo_id=model_id_or_path, allow_patterns=allow_patterns))


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


def probe_mlx_embedding(args: argparse.Namespace) -> dict[str, Any]:
    import mlx.core as mx
    import numpy as np
    from mlx_embeddings import load

    started = time.time()
    model_path = resolve_snapshot(args.embedding_model, QWEN3_VL_ALLOW_PATTERNS)
    model, processor = load(str(model_path))
    shims = patch_qwen3_vl_processor(processor)

    with tempfile.TemporaryDirectory(prefix="cerul-runtime-matrix-") as tmp:
        image_path = Path(tmp) / "sample.png"
        make_sample_image(image_path)
        embeddings = model.process(
            [
                {
                    "text": "Find clips where someone explains indexing progress.",
                    "instruction": "Retrieve images or text relevant to the user's query.",
                },
                {"text": "A dashboard shows local model runtime status."},
                {"image": str(image_path)},
            ],
            processor=processor,
        )
        mx.eval(embeddings)
        array = np.asarray(embeddings)

    finite = bool(np.isfinite(array).all())
    shape = list(array.shape)
    return {
        "ok": shape == [3, 2048] and finite,
        "product_role": "default embedding",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "shape": shape,
        "finite": finite,
        "dtype": str(array.dtype),
        "elapsed_seconds": round(time.time() - started, 3),
        "model_path": str(model_path),
        "compat_shims": shims,
    }


def probe_mlx_reranker(args: argparse.Namespace) -> dict[str, Any]:
    import mlx.core as mx
    import numpy as np
    from mlx_embeddings import load

    started = time.time()
    model_path = resolve_snapshot(args.reranker_model, QWEN3_VL_ALLOW_PATTERNS)
    model, processor = load(str(model_path))
    shims = patch_qwen3_vl_processor(processor)
    payload = {
        "instruction": "Retrieve passages that answer the query.",
        "query": {"text": "How does Cerul avoid CPU model inference?"},
        "documents": [
            {"text": "Cerul runs default model inference through MLX on Apple Silicon."},
            {"text": "The weather forecast is cloudy tomorrow."},
        ],
    }
    scores = model.process(payload, processor=processor)
    mx.eval(scores)
    array = np.asarray(scores)
    finite = bool(np.isfinite(array).all())
    return {
        "ok": list(array.shape) == [2] and finite and float(array[0]) > float(array[1]),
        "product_role": "recommended reranker",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "shape": list(array.shape),
        "finite": finite,
        "scores": [float(x) for x in array.tolist()],
        "elapsed_seconds": round(time.time() - started, 3),
        "model_path": str(model_path),
        "compat_shims": shims,
    }


def probe_mlx_text_reranker(args: argparse.Namespace) -> dict[str, Any]:
    from mlx_lm import generate, load

    started = time.time()
    model, tokenizer = load(args.text_reranker_model)
    query = "How does Cerul avoid CPU model inference?"
    documents = [
        "Cerul runs default model inference through MLX on Apple Silicon.",
        "The weather forecast is cloudy tomorrow.",
    ]
    outputs = []
    for document in documents:
        prompt = (
            "Given a query and a document, answer only yes or no whether the document is relevant.\n"
            f"Query: {query}\n"
            f"Document: {document}\n"
            "Relevant:"
        )
        outputs.append(generate(model, tokenizer, prompt=prompt, max_tokens=4, verbose=False))

    normalized = [output.strip().lower() for output in outputs]
    return {
        "ok": normalized[0].startswith("yes") and normalized[1].startswith("no"),
        "product_role": "lightweight text reranker candidate",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "outputs": outputs,
        "elapsed_seconds": round(time.time() - started, 3),
        "model": args.text_reranker_model,
    }


def segment_like(value: Any) -> bool:
    if isinstance(value, dict):
        return all(key in value for key in ("start", "end", "text"))
    return all(hasattr(value, key) for key in ("start", "end", "text"))


def probe_mlx_qwen3_asr(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    if args.audio is None:
        return {"ok": False, "skipped": True, "reason": "pass --audio to run ASR probe"}

    module = importlib.import_module("mlx_qwen3_asr")
    public = sorted(name for name in dir(module) if not name.startswith("_"))

    if hasattr(module, "transcribe"):
        result = module.transcribe(
            str(args.audio),
            model=args.qwen_asr_model,
            return_timestamps=True,
            forced_aligner=args.forced_aligner_model,
        )
    else:
        return {
            "ok": False,
            "error": "mlx_qwen3_asr does not expose a transcribe() function",
            "public_attributes": public,
        }

    text = getattr(result, "text", None) if not isinstance(result, dict) else result.get("text")
    segments = getattr(result, "segments", None) if not isinstance(result, dict) else result.get("segments")
    timestamped = isinstance(segments, list) and bool(segments) and all(segment_like(s) for s in segments)
    return {
        "ok": bool(text) and timestamped,
        "product_role": "ASR + timestamp alignment candidate",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "text_length": len(text or ""),
        "has_timestamped_segments": timestamped,
        "segment_count": len(segments) if isinstance(segments, list) else 0,
        "result_type": type(result).__name__,
        "public_attributes": public,
        "elapsed_seconds": round(time.time() - started, 3),
    }


def probe_mlx_whisper(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    if args.audio is None:
        return {"ok": False, "skipped": True, "reason": "pass --audio to run ASR probe"}

    import mlx_whisper

    output = mlx_whisper.transcribe(
        str(args.audio),
        path_or_hf_repo=args.mlx_whisper_model,
        word_timestamps=True,
    )
    segments = output.get("segments") or []
    words = [word for segment in segments for word in segment.get("words", [])]
    timestamped_segments = all(
        all(key in segment for key in ("start", "end", "text")) for segment in segments
    )
    timestamped_words = all(all(key in word for key in ("start", "end", "word")) for word in words)
    return {
        "ok": bool(output.get("text")) and bool(segments) and timestamped_segments,
        "product_role": "ASR fallback candidate",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "text_length": len(output.get("text", "")),
        "segment_count": len(segments),
        "word_count": len(words),
        "has_timestamped_segments": timestamped_segments,
        "has_timestamped_words": timestamped_words,
        "elapsed_seconds": round(time.time() - started, 3),
    }


def probe_whispermlx(_: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    try:
        module = importlib.import_module("whispermlx")
    except Exception as exc:  # noqa: BLE001 - matrix should report install/import problems.
        return {
            "ok": False,
            "product_role": "ASR alignment candidate",
            "target_runtime": "MLX",
            "cpu_inference": False,
            "error_type": type(exc).__name__,
            "error": str(exc),
            "elapsed_seconds": round(time.time() - started, 3),
        }
    import_results: dict[str, str] = {}
    for submodule in ("whispermlx.asr", "whispermlx.transcribe"):
        try:
            importlib.import_module(submodule)
            import_results[submodule] = "ok"
        except Exception as exc:  # noqa: BLE001 - dependency issues are the signal here.
            import_results[submodule] = f"{type(exc).__name__}: {exc}"

    ready = all(value == "ok" for value in import_results.values())
    return {
        "ok": ready,
        "product_role": "ASR alignment candidate",
        "target_runtime": "MLX Whisper plus Torch/Pyannote adjuncts",
        "cpu_inference": False,
        "eligible_for_product_default": False,
        "reason": (
            "whispermlx wraps mlx-whisper but imports Torch/Pyannote/Wav2Vec adjunct models "
            "for VAD/alignment; use direct mlx-whisper or mlx-qwen3-asr for the no-CPU default."
        ),
        "submodule_imports": import_results,
        "public_attributes": sorted(name for name in dir(module) if not name.startswith("_"))[:80],
        "elapsed_seconds": round(time.time() - started, 3),
    }


def probe_onnx_vad(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    if args.audio is None:
        return {"ok": False, "skipped": True, "reason": "pass --audio to run VAD probe"}

    import numpy as np
    ort = importlib.import_module("onnxruntime")
    import soundfile as sf
    from huggingface_hub import hf_hub_download

    model_path = hf_hub_download(args.vad_model, args.vad_onnx_file)
    session = ort.InferenceSession(model_path, providers=["CoreMLExecutionProvider"])
    providers = session.get_providers()
    audio, sample_rate = sf.read(str(args.audio), dtype="float32")
    if audio.ndim > 1:
        audio = audio.mean(axis=1)

    state = np.zeros((2, 1, 128), dtype=np.float32)
    probabilities: list[float] = []
    for offset in range(0, min(len(audio), sample_rate * 5), 512):
        chunk = audio[offset : offset + 512]
        if len(chunk) < 512:
            chunk = np.pad(chunk, (0, 512 - len(chunk)))
        output, state = session.run(
            None,
            {
                "input": chunk.reshape(1, -1).astype(np.float32),
                "state": state,
                "sr": np.array(sample_rate, dtype=np.int64),
            },
        )
        probabilities.append(float(output[0][0]))

    max_probability = max(probabilities) if probabilities else 0.0
    speech_frames = sum(probability > 0.5 for probability in probabilities)
    cpu_fallback_present = "CPUExecutionProvider" in providers
    coreml_present = "CoreMLExecutionProvider" in providers
    return {
        "ok": coreml_present and bool(probabilities) and max_probability > 0.5,
        "product_role": "VAD candidate",
        "target_runtime": "ONNX Runtime CoreML EP",
        "cpu_inference": not coreml_present,
        "cpu_fallback_provider_present": cpu_fallback_present,
        "available_providers": providers,
        "frame_count": len(probabilities),
        "speech_frames": speech_frames,
        "max_probability": round(max_probability, 6),
        "sample_rate": sample_rate,
        "elapsed_seconds": round(time.time() - started, 3),
        "model_path": model_path,
    }


def probe_mlx_ocr(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    from mlx_vlm import apply_chat_template, generate, load

    image_path = Path(".tmp/runtime-ocr-vlm.png")
    make_sample_image(image_path)
    model, processor = load(args.ocr_model)
    prompt = apply_chat_template(
        processor,
        model.config,
        "Read the text in this image. Return only the text.",
        num_images=1,
    )
    result = generate(
        model,
        processor,
        prompt=prompt,
        image=str(image_path),
        max_tokens=16,
        verbose=False,
    )
    text = getattr(result, "text", str(result)).strip()
    return {
        "ok": "CERUL" in text.upper(),
        "product_role": "OCR candidate",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "text": text,
        "image_path": str(image_path),
        "elapsed_seconds": round(time.time() - started, 3),
        "model": args.ocr_model,
    }


def probe_onnx_ocr(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    import numpy as np
    ort = importlib.import_module("onnxruntime")
    from huggingface_hub import hf_hub_download
    from PIL import Image

    image_path = Path(".tmp/runtime-ocr-onnx.png")
    make_sample_image(image_path)
    det_path = hf_hub_download(args.onnx_ocr_model, "ppocrv5-mobile-det.onnx")
    rec_path = hf_hub_download(args.onnx_ocr_model, "ppocrv5-mobile-rec.onnx")
    det_session = ort.InferenceSession(det_path, providers=["CoreMLExecutionProvider"])
    rec_session = ort.InferenceSession(rec_path, providers=["CoreMLExecutionProvider"])
    providers = sorted(set(det_session.get_providers()) | set(rec_session.get_providers()))

    image = Image.open(image_path).convert("RGB")
    det_image = image.resize((320, 64))
    det_array = np.asarray(det_image).astype("float32") / 255.0
    det_array = (det_array - [0.485, 0.456, 0.406]) / [0.229, 0.224, 0.225]
    det_array = det_array.transpose(2, 0, 1)[None].astype("float32")
    det_output = det_session.run(None, {"x": det_array})[0]

    rec_image = image.resize((320, 48))
    rec_array = np.asarray(rec_image).astype("float32") / 255.0
    rec_array = ((rec_array - 0.5) / 0.5).transpose(2, 0, 1)[None].astype("float32")
    rec_output = rec_session.run(None, {"x": rec_array})[0]
    coreml_present = "CoreMLExecutionProvider" in providers
    return {
        "ok": coreml_present
        and bool(np.isfinite(det_output).all())
        and bool(np.isfinite(rec_output).all()),
        "product_role": "OCR ONNX comparison candidate",
        "target_runtime": "ONNX Runtime CoreML EP",
        "cpu_inference": "CPUExecutionProvider" in providers,
        "available_providers": providers,
        "det_shape": list(det_output.shape),
        "rec_shape": list(rec_output.shape),
        "det_max": round(float(det_output.max()), 6),
        "elapsed_seconds": round(time.time() - started, 3),
        "note": "CoreML EP loads, but CPUExecutionProvider remains present for unsupported/fallback nodes; use mlx-ocr for no-CPU product path.",
    }


def probe_forced_aligner(args: argparse.Namespace) -> dict[str, Any]:
    started = time.time()
    if args.audio is None:
        return {"ok": False, "skipped": True, "reason": "pass --audio to run forced aligner probe"}

    import numpy as np
    import soundfile as sf
    from mlx_qwen3_asr import ForcedAligner

    transcript = "Test time compute improves reasoning when the model can think for."
    audio, sample_rate = sf.read(str(args.audio), dtype="float32")
    if audio.ndim > 1:
        audio = audio.mean(axis=1)

    aligner = ForcedAligner(args.forced_aligner_model)
    words = aligner.align(np.asarray(audio, dtype=np.float32), transcript, "English")
    timestamped = bool(words) and all(
        hasattr(word, "text") and hasattr(word, "start_time") and hasattr(word, "end_time")
        for word in words
    )
    return {
        "ok": timestamped,
        "product_role": "word timestamp alignment",
        "target_runtime": "MLX",
        "cpu_inference": False,
        "sample_rate": sample_rate,
        "word_count": len(words),
        "first_word": getattr(words[0], "text", None) if words else None,
        "first_start": getattr(words[0], "start_time", None) if words else None,
        "first_end": getattr(words[0], "end_time", None) if words else None,
        "elapsed_seconds": round(time.time() - started, 3),
    }


def run_command(command: list[str], timeout: int = 120) -> dict[str, Any]:
    started = time.time()
    try:
        completed = subprocess.run(
            command,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        return {
            "command": command,
            "exit_code": completed.returncode,
            "output": completed.stdout[-4000:],
            "elapsed_seconds": round(time.time() - started, 3),
        }
    except subprocess.TimeoutExpired as exc:
        return {
            "command": command,
            "exit_code": None,
            "timed_out": True,
            "output": (exc.stdout or "")[-4000:] if isinstance(exc.stdout, str) else "",
            "elapsed_seconds": round(time.time() - started, 3),
        }


def probe_server_infra(_: argparse.Namespace) -> dict[str, Any]:
    py_version = f"{sys.version_info.major}{sys.version_info.minor}"
    platform_tag = "macosx_14_0_arm64"
    with tempfile.TemporaryDirectory(prefix="cerul-server-infra-wheels-") as wheel_dir:
        checks = {
            "vllm_macos_arm64_wheel": run_command(
                [
                    sys.executable,
                    "-m",
                    "pip",
                    "download",
                    "--only-binary=:all:",
                    "--no-deps",
                    "--platform",
                    platform_tag,
                    "--python-version",
                    py_version,
                    "--implementation",
                    "cp",
                    "--abi",
                    f"cp{py_version}",
                    "--dest",
                    wheel_dir,
                    "vllm==0.21.0",
                ]
            ),
            "sglang_macos_arm64_wheel": run_command(
                [
                    sys.executable,
                    "-m",
                    "pip",
                    "download",
                    "--only-binary=:all:",
                    "--no-deps",
                    "--platform",
                    platform_tag,
                    "--python-version",
                    py_version,
                    "--implementation",
                    "cp",
                    "--abi",
                    f"cp{py_version}",
                    "--dest",
                    wheel_dir,
                    "sglang==0.5.10.post1",
                ]
            ),
        }
    return {
        "ok": False,
        "product_role": "server-side reference/runtime fallback",
        "target_runtime": "CUDA/server infra",
        "cpu_inference": False,
        "eligible_for_mac_default": False,
        "reason": "vLLM/SGLang are server inference stacks, not the Apple Silicon MLX product runtime.",
        "checks": checks,
    }


def capture(name: str, fn: Callable[[argparse.Namespace], dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    try:
        result = fn(args)
        result.setdefault("ok", False)
        return result
    except Exception as exc:  # noqa: BLE001 - matrix should preserve runtime failures.
        return {
            "ok": False,
            "error_type": type(exc).__name__,
            "error": str(exc),
            "probe": name,
        }


PROBE_FNS: dict[str, Callable[[argparse.Namespace], dict[str, Any]]] = {
    "mlx-embedding": probe_mlx_embedding,
    "mlx-reranker": probe_mlx_reranker,
    "mlx-text-reranker": probe_mlx_text_reranker,
    "mlx-qwen3-asr": probe_mlx_qwen3_asr,
    "mlx-whisper": probe_mlx_whisper,
    "whispermlx": probe_whispermlx,
    "onnx-vad": probe_onnx_vad,
    "mlx-ocr": probe_mlx_ocr,
    "onnx-ocr": probe_onnx_ocr,
    "forced-aligner": probe_forced_aligner,
    "server-infra": probe_server_infra,
}


def selected_probes(args: argparse.Namespace) -> list[str]:
    if args.probe:
        return args.probe
    return [
        "mlx-embedding",
        "mlx-text-reranker",
        "mlx-qwen3-asr",
        "mlx-whisper",
        "onnx-vad",
        "mlx-ocr",
        "forced-aligner",
    ]


def summarize(report: dict[str, Any], probes: list[str]) -> None:
    report["summary"] = {
        "all_selected_ok": all(report["probes"].get(name, {}).get("ok") for name in probes),
        "selected": probes,
        "ok": [name for name in probes if report["probes"].get(name, {}).get("ok")],
        "failed": [name for name in probes if not report["probes"].get(name, {}).get("ok")],
        "cpu_inference_detected": [
            name for name in probes if report["probes"].get(name, {}).get("cpu_inference")
        ],
    }


def markdown(report: dict[str, Any]) -> str:
    lines = [
        "# Model Runtime Matrix",
        "",
        f"Generated: {report['timestamp_utc']}",
        "",
        "| Probe | Product role | Runtime | OK | CPU inference | Notes |",
        "|---|---|---|---:|---:|---|",
    ]
    for name, result in report["probes"].items():
        if name == "prereqs":
            continue
        note = result.get("reason") or result.get("error") or result.get("note") or ""
        lines.append(
            "| {name} | {role} | {runtime} | {ok} | {cpu} | {note} |".format(
                name=name,
                role=result.get("product_role", ""),
                runtime=result.get("target_runtime", ""),
                ok="yes" if result.get("ok") else "no",
                cpu="yes" if result.get("cpu_inference") else "no",
                note=str(note).replace("\n", " ")[:180],
            )
        )
    lines.append("")
    lines.append("## Summary")
    lines.append("")
    lines.append(f"- OK: {', '.join(report['summary'].get('ok', [])) or 'none'}")
    lines.append(f"- Failed: {', '.join(report['summary'].get('failed', [])) or 'none'}")
    lines.append(
        f"- CPU inference detected: {', '.join(report['summary'].get('cpu_inference_detected', [])) or 'none'}"
    )
    return "\n".join(lines) + "\n"


def write_outputs(report: dict[str, Any], json_path: Path | None, markdown_path: Path | None) -> None:
    text = json.dumps(report, indent=2, sort_keys=True)
    if json_path:
        json_path.parent.mkdir(parents=True, exist_ok=True)
        json_path.write_text(text + "\n", encoding="utf-8")
    if markdown_path:
        markdown_path.parent.mkdir(parents=True, exist_ok=True)
        markdown_path.write_text(markdown(report), encoding="utf-8")
    print(text)


def isolated_child_args(args: argparse.Namespace, probe: str, report_path: Path) -> list[str]:
    command = [sys.executable, __file__, "--probe", probe, "--no-isolate", "--report", str(report_path)]

    path_options = {
        "--models-cache": args.models_cache,
        "--audio": args.audio,
    }
    string_options = {
        "--embedding-model": args.embedding_model,
        "--reranker-model": args.reranker_model,
        "--text-reranker-model": args.text_reranker_model,
        "--qwen-asr-model": args.qwen_asr_model,
        "--mlx-whisper-model": args.mlx_whisper_model,
        "--forced-aligner-model": args.forced_aligner_model,
        "--ocr-model": args.ocr_model,
        "--vad-model": args.vad_model,
        "--vad-onnx-file": args.vad_onnx_file,
        "--onnx-ocr-model": args.onnx_ocr_model,
    }
    for option, value in path_options.items():
        if value is not None:
            command.extend([option, str(value)])
    for option, value in string_options.items():
        command.extend([option, str(value)])
    return command


def run_isolated_probes(args: argparse.Namespace, report: dict[str, Any], probes: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="cerul-runtime-matrix-") as temp_dir:
        temp_path = Path(temp_dir)
        for name in probes:
            child_report_path = temp_path / f"{name}.json"
            child = run_command(
                isolated_child_args(args, name, child_report_path),
                timeout=1800,
            )
            if child_report_path.exists():
                child_report = json.loads(child_report_path.read_text(encoding="utf-8"))
                report["probes"][name] = child_report["probes"].get(name, {})
            else:
                report["probes"][name] = {
                    "ok": False,
                    "probe": name,
                    "error": "isolated probe process did not produce a report",
                    "child": child,
                }
            report["probes"][name].setdefault("ok", False)


def main() -> int:
    args = parse_args()
    cache_env = configure_cache(args.models_cache)
    report = base_report(args, cache_env)
    prereqs_ok = check_prereqs(args, report)

    probes = selected_probes(args)
    if args.check_prereqs:
        summarize(report, [])
        write_outputs(report, args.report, args.markdown_report)
        return 0 if prereqs_ok else 2

    if not prereqs_ok:
        summarize(report, probes)
        write_outputs(report, args.report, args.markdown_report)
        return 2

    if len(probes) > 1 and not args.probe and not args.no_isolate:
        run_isolated_probes(args, report, probes)
    else:
        for name in probes:
            report["probes"][name] = capture(name, PROBE_FNS[name], args)

    summarize(report, probes)
    write_outputs(report, args.report, args.markdown_report)

    if args.strict and not report["summary"]["all_selected_ok"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
