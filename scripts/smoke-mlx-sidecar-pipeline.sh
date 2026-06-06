#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEFAULT_PYTHON="$ROOT/.tmp/runtime-matrix-venv/bin/python"
if [[ -z "${CERUL_MLX_PYTHON:-}" && -x "$DEFAULT_PYTHON" ]]; then
  export CERUL_MLX_PYTHON="$DEFAULT_PYTHON"
fi

export CERUL_MLX_SIDECAR="${CERUL_MLX_SIDECAR:-$ROOT/mlx-sidecar/cerul_mlx_sidecar.py}"
export CERUL_MLX_MODELS_CACHE="${CERUL_MLX_MODELS_CACHE:-$ROOT/.tmp/runtime-models}"
export CERUL_MLX_SMOKE_WAV="${CERUL_MLX_SMOKE_WAV:-$ROOT/.tmp/real-index-data/cache/audio/itm_5982fccb0c72.wav}"

if [[ ! -f "$CERUL_MLX_SMOKE_WAV" ]]; then
  echo "CERUL_MLX_SMOKE_WAV does not exist: $CERUL_MLX_SMOKE_WAV" >&2
  echo "Provide a 16 kHz mono speech WAV to run the real MLX indexing smoke." >&2
  exit 2
fi

"${CERUL_MLX_PYTHON:-python3}" -u "$CERUL_MLX_SIDECAR" \
  --models-cache "$CERUL_MLX_MODELS_CACHE" <<<'{"id":1,"method":"status","params":{}}'

cargo test -p cerul-pipeline mlx_sidecar_video_pipeline_smoke_indexes_video -- --ignored --nocapture
cargo test -p cerul-api mlx_sidecar_default_worker_smoke_indexes_added_folder_video -- --ignored --nocapture
