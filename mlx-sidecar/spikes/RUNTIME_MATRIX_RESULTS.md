# Runtime Matrix Results

Date: 2026-05-22

This is a historical result from the earlier local-runtime matrix. The active
v1 development path now uses API-first providers, so the old matrix wrapper is
no longer part of the repo.

The default product matrix runs probes in isolated Python processes. A single
process can segfault after loading several MLX/VLM/Whisper models back to back;
the product runtime should keep the same isolation boundary.

## Product Default Matrix

| Capability | Model | Runtime | Result |
|---|---|---|---|
| Embedding | `mlx-community/Qwen3-VL-Embedding-2B-6bit` | MLX | pass, `[3, 2048]`, finite, `float16` |
| Reranking | `mlx-community/Qwen3-Reranker-0.6B-mxfp8` | MLX | pass, relevant doc answered `yes`, irrelevant doc answered `no` |
| ASR | `Qwen/Qwen3-ASR-0.6B` + `Qwen/Qwen3-ForcedAligner-0.6B` | MLX | pass, 11 timestamped segments |
| ASR fallback | `mlx-community/whisper-large-v3-turbo` | MLX | pass, segment and word timestamps |
| VAD | `onnx-community/silero-vad` `onnx/model_quantized.onnx` | ONNX Runtime CoreML EP | pass, 122 frames, max speech probability `0.999637` |
| OCR | `mlx-community/Qwen3-VL-2B-Instruct-4bit` | MLX | pass, synthetic image read as `CERUL` |
| Forced aligner | `Qwen/Qwen3-ForcedAligner-0.6B` | MLX | pass, 11 words aligned |

Summary: all default product probes passed and `cpu_inference_detected` was
empty.

## Rejected Or Non-Default Candidates

| Candidate | Result | Reason |
|---|---|---|
| `Qwen/Qwen3-VL-Reranker-2B` via `mlx-embeddings==0.1.0` | fail | RuntimeError: `Item size 2 for PEP 3118 buffer format string B does not match the dtype B item size 1`; also too heavy for the first local default. |
| `whispermlx==3.12.1` | fail for product default | Wraps `mlx-whisper`, but imports Torch/Pyannote/Wav2Vec adjuncts for VAD/alignment. Direct `mlx-whisper` and `mlx-qwen3-asr` are cleaner no-CPU defaults. |
| `vllm==0.21.0` | fail for local Mac default | No macOS arm64 wheel. |
| `sglang==0.5.10.post1` | not eligible | Has a Python wheel, but it is server/CUDA infra, not the Apple Silicon product runtime. |
| PaddleOCR PP-OCRv5 mobile ONNX | comparison only | Model files load and run, but ONNX Runtime keeps CPU fallback provider present. Use MLX OCR for the no-CPU product path. |

## Historical Product Code Implications

These implications applied to the earlier local-runtime track. They are kept as
evidence for future local-model work, but they are not the active v1 defaults.

- The local-runtime track favored Qwen3-ASR + Qwen3 ForcedAligner over
  whisper.cpp.
- The local-runtime track favored moving embedding to an MLX sidecar and gating
  the Rust fastembed CPU path behind `CERUL_ALLOW_LEGACY_CPU_EMBEDDING=1`.
- Legacy Whisper auto-download was rejected for the local-runtime default unless
  the user explicitly selected a fallback ASR model.
- `TextOnlyFallbackEmbedder` / zero-vector indexing was rejected for default
  worker behavior.
