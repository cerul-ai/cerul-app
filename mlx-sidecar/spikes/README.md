# MLX P0 Spike

This directory contains the feasibility spike for the original Apple Silicon
(MLX) local-model runtime P0.

This is a historical spike record. The repository no longer ships active MLX
spike wrapper scripts because v1 development is API-first.

The spike validates three things:

- `mlx-embeddings` can load `mlx-community/Qwen3-VL-Embedding-2B-6bit`.
- Text and image embeddings are finite 2048-dimensional vectors.
- `qwen3-asr-mlx` can load a Qwen3-ASR model and whether its public result
  shape includes timestamped segments required by Cerul's pipeline.

`torch` and `torchvision` are included because `mlx-embeddings 0.1.0` routes
Qwen3-VL processor initialization through Transformers' `AutoImageProcessor`.
The actual embedding inference remains MLX-backed; P0 records this packaging
dependency explicitly because it affects the future sidecar bundle size.

See `RUNTIME_MATRIX_RESULTS.md` for the historical local-runtime pass/fail
matrix and rejected candidates.
