#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
API_PORT="${CERUL_API_PORT:-7777}"

if [ "${CERUL_SKIP_DEV_RUNTIME_CLEANUP:-0}" = "1" ]; then
  exit 0
fi

host_target_triple() {
  local arch
  arch="$(uname -m)"
  case "$(uname -s)" in
    Darwin)
      if [ "$arch" = "arm64" ]; then
        echo "aarch64-apple-darwin"
      else
        echo "x86_64-apple-darwin"
      fi
      ;;
    Linux)
      if [ "$arch" = "aarch64" ] || [ "$arch" = "arm64" ]; then
        echo "aarch64-unknown-linux-gnu"
      else
        echo "x86_64-unknown-linux-gnu"
      fi
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "x86_64-pc-windows-msvc"
      ;;
    *)
      echo "$arch-unknown"
      ;;
  esac
}

terminate_pid() {
  local pid="$1"
  local label="$2"
  if ! kill -0 "$pid" 2>/dev/null; then
    return
  fi
  echo "Stopping orphan $label process pid=$pid" >&2
  kill "$pid" 2>/dev/null || true
  for _ in 1 2 3 4 5; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return
    fi
    sleep 0.2
  done
  echo "Force stopping orphan $label process pid=$pid" >&2
  kill -9 "$pid" 2>/dev/null || true
}

kill_orphan_processes_for_path() {
  local label="$1"
  local executable="$2"
  if [ ! -e "$executable" ]; then
    return
  fi

  ps -axo pid=,ppid=,command= | while read -r pid ppid command; do
    if [ -z "${pid:-}" ] || [ -z "${ppid:-}" ]; then
      continue
    fi
    if [ "$ppid" != "1" ]; then
      continue
    fi
    if [[ "$command" == "$executable"* ]]; then
      terminate_pid "$pid" "$label"
    fi
  done
}

check_api_port() {
  if ! command -v lsof >/dev/null 2>&1; then
    return
  fi

  local pids
  pids="$(lsof -tiTCP:"$API_PORT" -sTCP:LISTEN 2>/dev/null || true)"
  if [ -z "$pids" ]; then
    return
  fi

  echo "Port $API_PORT is already in use:" >&2
  lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >&2 || true

  if command -v curl >/dev/null 2>&1 &&
    curl -fsS --max-time 1 "http://127.0.0.1:$API_PORT/health" >/dev/null 2>&1; then
    echo "Existing Cerul API on port $API_PORT is healthy; Electron will reuse it." >&2
    return
  fi

  echo "Port $API_PORT is occupied but Cerul health is not reachable. Stop that process before running Cerul." >&2
  exit 1
}

API_BINARY="$ROOT/target/debug/cerul-api"
TARGET_TRIPLE="$(host_target_triple)"
QDRANT_BINARY="$ROOT/third-party/$TARGET_TRIPLE/qdrant"
if [[ "$TARGET_TRIPLE" == *windows* ]]; then
  QDRANT_BINARY="$QDRANT_BINARY.exe"
fi

kill_orphan_processes_for_path "Cerul API" "$API_BINARY"
kill_orphan_processes_for_path "Qdrant" "$QDRANT_BINARY"
check_api_port
