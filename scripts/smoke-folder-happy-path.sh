#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test -p cerul-pipeline folder_happy_path_smoke -- --ignored --nocapture
