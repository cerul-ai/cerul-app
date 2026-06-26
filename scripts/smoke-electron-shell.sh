#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

source scripts/load-env.sh

API_PORT="${CERUL_API_PORT:-23785}"
export CERUL_API_PORT="$API_PORT"
API_HEALTH_URL="http://127.0.0.1:$API_PORT/internal/health"

if command -v lsof >/dev/null 2>&1 && lsof -tiTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Port $API_PORT is already in use; stop the existing Cerul Core before running Electron shell smoke." >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
LOG_FILE="$TMP_DIR/electron.log"
TIMEOUT_SECONDS="${CERUL_ELECTRON_SMOKE_TIMEOUT:-180}"
export CERUL_DATA_DIR="$TMP_DIR/data"
export ELECTRON_ENABLE_LOGGING=1

cleanup() {
  local status=$?
  if [ -n "${ELECTRON_PID:-}" ] && kill -0 "$ELECTRON_PID" 2>/dev/null; then
    kill "$ELECTRON_PID" 2>/dev/null || true
    wait "$ELECTRON_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT INT TERM

pnpm --filter @cerul/desktop build >/dev/null
cargo build -p cerul-api >/dev/null
pnpm --filter @cerul/electron-shell build >/dev/null

pnpm --filter @cerul/electron-shell start >"$LOG_FILE" 2>&1 &
ELECTRON_PID=$!

HEALTH=""
for _ in $(seq 1 $((TIMEOUT_SECONDS * 2))); do
  if curl -fsS "$API_HEALTH_URL" >/dev/null 2>&1; then
    HEALTH="$(curl -fsS "$API_HEALTH_URL")"
    if grep -q "cerul_electron_main_window_loaded" "$LOG_FILE"; then
      echo "electron_shell_smoke status=ok health=$HEALTH main_window=loaded data_dir=$CERUL_DATA_DIR"
      exit 0
    fi
  fi
  if ! kill -0 "$ELECTRON_PID" 2>/dev/null; then
    echo "Electron exited before Cerul Core became healthy." >&2
    sed -n '1,200p' "$LOG_FILE" >&2 || true
    exit 1
  fi
  sleep 0.5
done

if [ -n "$HEALTH" ]; then
  echo "Timed out waiting for Electron main window to load after ${TIMEOUT_SECONDS}s." >&2
else
  echo "Timed out waiting for Electron-started Cerul Core after ${TIMEOUT_SECONDS}s." >&2
fi
sed -n '1,200p' "$LOG_FILE" >&2 || true
exit 1
