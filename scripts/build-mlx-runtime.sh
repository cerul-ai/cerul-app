#!/usr/bin/env bash
set -euo pipefail

# Build the self-contained, relocatable Python runtime that ships INSIDE the
# packaged app so on-device (MLX) models run from a clean install with no user
# setup. Output: apps/electron-shell/mlx-runtime/ (bin/python3 + full stdlib +
# the locked MLX stack). electron-builder copies it to
# Contents/Resources/mlx-runtime, and main.ts points CERUL_MLX_PYTHON at it.
#
# Approach: uv fetches a python-build-standalone CPython (relocatable by
# design); we drop the PEP-668 marker and install the FLATTENED lock with
# --no-deps so the set is exactly the verified runtime — no torch / datasets /
# audio-gen extras (~900 MB lighter than a naive `pip install -r requirements`).
#
# Usage: scripts/build-mlx-runtime.sh [--python 3.12]

PY_VERSION="3.12"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --python) PY_VERSION="${2:?missing version}"; shift 2 ;;
    -h|--help) sed -n '3,17p' "$0"; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "build-mlx-runtime.sh currently targets macOS only." >&2
  exit 1
fi
if ! command -v uv >/dev/null 2>&1; then
  echo "uv is required (https://docs.astral.sh/uv/). Install it and retry." >&2
  exit 1
fi

LOCK="$ROOT/mlx-sidecar/runtime-lock.txt"
RUNTIME_DIR="$ROOT/apps/electron-shell/mlx-runtime"
STAGE="$ROOT/.tmp/mlx-runtime-stage"

echo "==> Fetching standalone CPython ${PY_VERSION} via uv"
rm -rf "$STAGE"
mkdir -p "$STAGE/pythons"
UV_PYTHON_INSTALL_DIR="$STAGE/pythons" uv python install "$PY_VERSION"
PYSRC="$(ls -d "$STAGE/pythons"/cpython-"${PY_VERSION}".*-macos-*/ 2>/dev/null | head -1)"
if [ -z "${PYSRC:-}" ] || [ ! -x "${PYSRC%/}/bin/python3" ]; then
  echo "Could not locate the installed standalone CPython under $STAGE/pythons" >&2
  exit 1
fi
PYSRC="${PYSRC%/}"

echo "==> Staging runtime at $RUNTIME_DIR"
rm -rf "$RUNTIME_DIR"
mkdir -p "$(dirname "$RUNTIME_DIR")"
cp -R "$PYSRC" "$RUNTIME_DIR"
PY="$RUNTIME_DIR/bin/python3"

# Drop the uv "externally managed" marker so the bundled python's own pip can
# install into it (these python-build-standalone builds are redistributable).
find "$RUNTIME_DIR/lib" -name "EXTERNALLY-MANAGED" -delete 2>/dev/null || true

echo "==> Installing locked MLX stack (--no-deps, exact set)"
"$PY" -m pip install --no-deps --no-input --disable-pip-version-check -r "$LOCK"

echo "==> Pruning bytecode caches + unused stdlib (GUI/test/dev tooling)"
find "$RUNTIME_DIR" -type d -name "__pycache__" -prune -exec rm -rf {} + 2>/dev/null || true
find "$RUNTIME_DIR" -name "*.pyc" -delete 2>/dev/null || true
# The sidecar is headless and never uses Tk, IDLE, the test suite, 2to3, or the
# C headers — dropping them trims size and the code-signing surface.
STDLIB="$RUNTIME_DIR/lib/python3.12"
rm -rf "$STDLIB/tkinter" "$STDLIB/idlelib" "$STDLIB/turtledemo" "$STDLIB/lib2to3" \
  "$STDLIB/test" "$STDLIB"/*/tests "$STDLIB"/site-packages/*/tests 2>/dev/null || true
rm -f "$STDLIB"/lib-dynload/_tkinter*.so "$STDLIB"/turtle.py 2>/dev/null || true
rm -rf "$RUNTIME_DIR"/lib/libtcl*.dylib "$RUNTIME_DIR"/lib/libtk*.dylib \
  "$RUNTIME_DIR"/lib/tcl* "$RUNTIME_DIR"/lib/tk* "$RUNTIME_DIR"/lib/libtcl9thread*.dylib \
  "$RUNTIME_DIR"/lib/thread3* "$RUNTIME_DIR"/lib/itcl* 2>/dev/null || true
rm -rf "$RUNTIME_DIR/include" "$RUNTIME_DIR/share" 2>/dev/null || true

echo "==> Verifying the runtime is self-contained, relocatable, and complete"
"$PY" - <<'PY'
import sys
assert sys.prefix.endswith("mlx-runtime"), f"not relocated cleanly: {sys.prefix}"
import mlx.core, mlx_vlm, mlx_lm, mlx_whisper, mlx_embeddings, mlx_qwen3_asr  # noqa: F401
import numpy, PIL, soundfile, huggingface_hub  # noqa: F401
from mlx_whisper import transcribe  # noqa: F401
from mlx_vlm import load  # noqa: F401
assert "torch" not in sys.modules, "torch should never load"
from importlib.metadata import version
print("  mlx", version("mlx"), "| python", sys.version.split()[0], "| imports OK, torch absent")
PY

echo "==> Done. Runtime size:"
du -sh "$RUNTIME_DIR"
