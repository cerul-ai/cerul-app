#!/usr/bin/env python3
"""Long-lived MLX runtime sidecar for Cerul local indexing.

The sidecar speaks newline-delimited JSON over stdin/stdout. Rust owns process
startup and request ordering; Python owns MLX model loading and inference.
Protocol responses always use stdout, while third-party library output is
redirected to stderr so it cannot corrupt the JSON stream.
"""

from __future__ import annotations

import argparse
import contextlib
import gc
import importlib.metadata
import json
import os
import platform
import sys
import threading
import time
import traceback
import unicodedata
from pathlib import Path
from typing import Any


DEFAULT_EMBEDDING_MODEL = "mlx-community/Qwen3-VL-Embedding-2B-6bit"
DEFAULT_ASR_MODEL = "Qwen/Qwen3-ASR-0.6B"
DEFAULT_FORCED_ALIGNER_MODEL = "Qwen/Qwen3-ForcedAligner-0.6B"
DEFAULT_OCR_MODEL = "mlx-community/Qwen3-VL-2B-Instruct-4bit"
DEFAULT_WHISPER_MODEL = "mlx-community/whisper-large-v3-turbo"
DEFAULT_TEXT_EMBED_BATCH_SIZE = 8
DEFAULT_IMAGE_EMBED_BATCH_SIZE = 2

QWEN3_VL_ALLOW_PATTERNS = [
    "*.json",
    "*.safetensors",
    "*.py",
    "*.tiktoken",
    "*.txt",
    "*.model",
    "*.jinja",
]

ORIGINAL_STDOUT = sys.stdout
_STDOUT_LOCK = threading.Lock()


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
    parser.add_argument("--ocr-model", default=DEFAULT_OCR_MODEL)
    parser.add_argument("--whisper-model", default=DEFAULT_WHISPER_MODEL)
    return parser.parse_args()


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def configure_cache(models_cache: Path) -> None:
    models_cache = models_cache.resolve()
    hf_home = models_cache / "huggingface"
    hf_home.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("HF_HOME", str(hf_home))


def env_positive_int(name: str, fallback: int) -> int:
    try:
        value = int(os.environ.get(name, ""))
    except ValueError:
        return fallback
    return max(1, value)


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


class CerulMlxRuntime:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.embedding_model = None
        self.embedding_processor = None
        self.embedding_model_path: str | None = None
        self.embedding_shims: list[str] = []
        self.ocr_model = None
        self.ocr_processor = None
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
        self.ocr_model = None
        self.ocr_processor = None
        self._clear_accelerator_cache()

    def release_transcription_runtime(self) -> None:
        self._asr_model_obj = None
        self._asr_aligner_obj = None
        self._clear_accelerator_cache()

    def release_models(self, scope: str = "all") -> dict[str, Any]:
        normalized = scope.strip().lower()
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
            "ocr": self.ocr_model is not None,
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
            "Pillow": package_version("Pillow"),
            "huggingface-hub": package_version("huggingface-hub"),
        }
        required = ["mlx", "mlx-embeddings", "mlx-qwen3-asr", "mlx-vlm"]
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
                "ocr": self.args.ocr_model,
            },
            "cache": {"HF_HOME": os.environ.get("HF_HOME")},
            "loaded": self.loaded_state(),
        }

    def load_embedding(self) -> None:
        if self.embedding_model is not None:
            return
        from mlx_embeddings import load

        model_path = resolve_snapshot(self.args.embedding_model, QWEN3_VL_ALLOW_PATTERNS)
        self.embedding_model, self.embedding_processor = load(str(model_path))
        self.embedding_model_path = str(model_path)
        self.embedding_shims = patch_qwen3_vl_processor(self.embedding_processor)

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
        if self.args.asr_model == "whisper-large-v3-turbo":
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

            kwargs: dict[str, Any] = {
                "path_or_hf_repo": self.args.whisper_model,
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
                "model": self.args.whisper_model,
            }
        finally:
            self.release_transcription_runtime()

    def load_ocr(self) -> None:
        if self.ocr_model is not None:
            return
        from mlx_vlm import load

        self.ocr_model, self.ocr_processor = load(self.args.ocr_model)

    def ocr_images(self, paths: list[str], prompt: str | None = None) -> dict[str, Any]:
        from mlx_vlm import apply_chat_template, generate

        self.load_ocr()
        prompt_text = prompt or "Read visible text in this frame. Return only the text."
        results = []
        for path in paths:
            prompt_payload = apply_chat_template(
                self.ocr_processor,
                self.ocr_model.config,
                prompt_text,
                num_images=1,
            )
            result = generate(
                self.ocr_model,
                self.ocr_processor,
                prompt=prompt_payload,
                image=path,
                max_tokens=48,
                verbose=False,
            )
            text = getattr(result, "text", str(result)).strip()
            results.append({"path": path, "text": text})
        return {"results": results, "model": self.args.ocr_model}


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
    args = parse_args()
    configure_cache(args.models_cache)
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
