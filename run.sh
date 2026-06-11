#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

source scripts/load-env.sh
export GGML_NATIVE="${GGML_NATIVE:-OFF}"
pnpm --filter @cerul/electron-shell dev
