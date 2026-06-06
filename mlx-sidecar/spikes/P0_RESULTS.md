# P0 Results — MLX Embedding and Qwen3-ASR

Date: 2026-05-22

This is a historical result from the earlier local-MLX track. The active v1
development path now uses API-first providers, so the old spike wrapper is no
longer part of the repo.

## Environment

- macOS 26.2 arm64
- Python 3.11.5
- `mlx==0.31.2`
- `mlx-embeddings==0.1.0`
- `qwen3-asr-mlx==0.1.0`
- `torch==2.12.0`
- `torchvision==0.27.0`

## Embedding Probe

Result: pass with recorded compatibility shims.

- Model: `mlx-community/Qwen3-VL-Embedding-2B-6bit`
- Output shape: `[3, 2048]`
- Output dtype: `float16`
- Output finite: `true`
- Elapsed after cached model load: `2.685s`

Compatibility findings:

- `mlx-embeddings 0.1.0` downloads Qwen3-VL snapshots with an allowlist that excludes `chat_template.jinja`; the P0 harness downloads `*.jinja` explicitly before calling `mlx_embeddings.load`.
- `mlx-embeddings 0.1.0` constructs `Qwen3VLProcessor` without running Transformers' processor initializer, so the P0 harness sets `image_ids`, `video_ids`, and `audio_ids` before calling `model.process(...)`.
- Qwen3-VL processor initialization also requires `torch` and `torchvision` even though embedding inference is MLX-backed; this affects the eventual sidecar package size.

## ASR Probe

Result: fail for the selected P0 model/runtime pair.

- Model: `mlx-community/Qwen3-ASR-0.6B-bf16`
- Runtime package: `qwen3-asr-mlx==0.1.0`
- Failure: model load raises `Missing 96 parameters` for `layers.18` through `layers.23`.

Root cause from local package inspection:

- The downloaded 0.6B config declares an 18-layer audio encoder under `thinker_config.audio_config`.
- `qwen3-asr-mlx 0.1.0` does not read that nested config shape and falls back to a 24-layer audio encoder default, so it expects six extra layers that are not in the 0.6B checkpoint.

Timestamp finding:

- P0 did not confirm timestamped ASR segments. The installed package's public result shape is `TranscriptionResult { text, language, duration }`, so even after the 0.6B load issue is fixed, Cerul still needs an explicit segment/timestamp strategy before P4.

## Decision

P0 green-lights the Qwen3-VL embedding direction only with the compatibility fixes above. It does not green-light the current Qwen3-ASR-0.6B + `qwen3-asr-mlx` path.

P1/P4 should not assume Qwen3-ASR-0.6B is ready until one of these is done:

- patch or fork `qwen3-asr-mlx` to read the 0.6B nested config and expose timestamped segments,
- choose another MLX ASR runtime that returns `Segment { start, end, text }`,
- or keep Whisper as the timestamp source while Qwen3-ASR remains text-only.

## Follow-up Runtime Matrix

The expanded matrix in `RUNTIME_MATRIX_RESULTS.md` supersedes the ASR failure
above for product planning: `mlx-qwen3-asr==0.3.5` can transcribe
`Qwen/Qwen3-ASR-0.6B`, and `Qwen/Qwen3-ForcedAligner-0.6B` provides timestamped
segments. The original P0 result remains useful only as evidence that
`qwen3-asr-mlx==0.1.0` should not be used.
