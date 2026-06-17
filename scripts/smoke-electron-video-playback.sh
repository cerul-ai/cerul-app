#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

source scripts/load-env.sh

VIDEO_PATH="${1:-$HOME/sample/video/moshi.mp4}"
TIMEOUT_SECONDS="${CERUL_ELECTRON_VIDEO_SMOKE_TIMEOUT:-1800}"
DEFAULT_PYTHON="$ROOT/.tmp/runtime-matrix-venv/bin/python"

binary_runs() {
  local binary="$1"
  shift
  [ -x "$binary" ] || return 1
  node - "$binary" "$@" <<'NODE'
const { spawnSync } = require("node:child_process");
const [, , binary, ...args] = process.argv;
const result = spawnSync(binary, args, { stdio: "ignore", timeout: 8000 });
process.exit(result.status === 0 ? 0 : 1);
NODE
}

host_target_triple() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "unsupported" ;;
  esac
}

if [ ! -f "$VIDEO_PATH" ]; then
  echo "sample video does not exist: $VIDEO_PATH" >&2
  exit 2
fi

if [ -z "${CERUL_MLX_PYTHON:-}" ] && [ -x "$DEFAULT_PYTHON" ]; then
  export CERUL_MLX_PYTHON="$DEFAULT_PYTHON"
fi
export CERUL_MLX_SIDECAR="${CERUL_MLX_SIDECAR:-$ROOT/mlx-sidecar/cerul_mlx_sidecar.py}"
export CERUL_MLX_MODELS_CACHE="${CERUL_MLX_MODELS_CACHE:-$ROOT/.tmp/runtime-models}"

HOST_TRIPLE="${CERUL_TARGET_TRIPLE:-$(host_target_triple)}"
if [ "$HOST_TRIPLE" = "unsupported" ]; then
  echo "Unsupported host for bundled binary lookup: $(uname -s)-$(uname -m)" >&2
  exit 2
fi
EXE_SUFFIX=""
case "$HOST_TRIPLE" in
  *windows*) EXE_SUFFIX=".exe" ;;
esac

if [ -z "${CERUL_FFMPEG_PATH:-}" ]; then
  BUNDLED_FFMPEG="$ROOT/third-party/$HOST_TRIPLE/ffmpeg$EXE_SUFFIX"
  if binary_runs "$BUNDLED_FFMPEG" -version; then
    export CERUL_FFMPEG_PATH="$BUNDLED_FFMPEG"
  elif command -v ffmpeg >/dev/null 2>&1 && binary_runs "$(command -v ffmpeg)" -version; then
    export CERUL_FFMPEG_PATH="$(command -v ffmpeg)"
  fi
fi
if [ -z "${CERUL_QDRANT_BIN:-}" ] && binary_runs "$ROOT/third-party/$HOST_TRIPLE/qdrant$EXE_SUFFIX" --version; then
  export CERUL_QDRANT_BIN="$ROOT/third-party/$HOST_TRIPLE/qdrant$EXE_SUFFIX"
fi

if [ -z "${CERUL_MLX_PYTHON:-}" ]; then
  echo "CERUL_MLX_PYTHON is not set and $DEFAULT_PYTHON is missing." >&2
  exit 2
fi
if [ -z "${CERUL_FFMPEG_PATH:-}" ]; then
  echo "No runnable ffmpeg was found. Set CERUL_FFMPEG_PATH or run scripts/fetch-binaries.sh with a standalone ffmpeg build." >&2
  exit 2
fi

if command -v lsof >/dev/null 2>&1 && lsof -tiTCP:7777 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Port 7777 is already in use; stop the existing Cerul Core before running Electron video smoke." >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
API_LOG="$TMP_DIR/cerul-core.log"
ELECTRON_LOG="$TMP_DIR/electron.log"
export CERUL_DATA_DIR="$TMP_DIR/data"
export ELECTRON_ENABLE_LOGGING=1

cleanup() {
  local status=$?
  if [ -n "${ELECTRON_PID:-}" ] && kill -0 "$ELECTRON_PID" 2>/dev/null; then
    kill "$ELECTRON_PID" 2>/dev/null || true
    wait "$ELECTRON_PID" 2>/dev/null || true
  fi
  if [ -n "${API_PID:-}" ] && kill -0 "$API_PID" 2>/dev/null; then
    kill "$API_PID" 2>/dev/null || true
    wait "$API_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT INT TERM

cargo build -p cerul-api >/dev/null
pnpm --filter @cerul/desktop build >/dev/null
pnpm --filter @cerul/electron-shell build >/dev/null

target/debug/cerul-api >"$API_LOG" 2>&1 &
API_PID=$!

request() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  if [ -n "$body" ]; then
    curl -fsS -X "$method" "http://127.0.0.1:7777$path" \
      -H 'content-type: application/json' \
      --data "$body"
  else
    curl -fsS -X "$method" "http://127.0.0.1:7777$path"
  fi
}

json_field() {
  local json="$1"
  local expr="$2"
  node -e "const data=JSON.parse(process.argv[1]); const value=($expr); if (value === undefined || value === null) process.exit(3); process.stdout.write(String(value));" "$json"
}

API_READY=0
for _ in $(seq 1 120); do
  if request GET /health >/dev/null 2>&1; then
    API_READY=1
    break
  fi
  if ! kill -0 "$API_PID" 2>/dev/null; then
    echo "Cerul Core exited before health became ready." >&2
    sed -n '1,200p' "$API_LOG" >&2 || true
    exit 1
  fi
  sleep 0.5
done
if [ "$API_READY" != "1" ]; then
  echo "Cerul Core did not become healthy before the Electron video smoke timeout." >&2
  sed -n '1,240p' "$API_LOG" >&2 || true
  exit 1
fi

CATALOG="$(request GET /models/catalog)"
LOCAL_READY="$(json_field "$CATALOG" 'data.runtime.local_runtime_ready')"
if [ "$LOCAL_READY" != "true" ]; then
  echo "local MLX runtime is not ready." >&2
  echo "$CATALOG" >&2
  sed -n '1,200p' "$API_LOG" >&2 || true
  exit 1
fi

request PATCH /settings '{"inference_mode":"local","indexing_paused":false}' >/dev/null
SOURCE_BODY="$(node -e "process.stdout.write(JSON.stringify({type:'file_video',config:{path:process.argv[1]}}))" "$VIDEO_PATH")"
SOURCE="$(request POST /sources "$SOURCE_BODY")"
SOURCE_ID="$(json_field "$SOURCE" 'data.id')"
ITEMS="$(request GET /items)"
ITEM_ID="$(node -e "const items=JSON.parse(process.argv[1]); const item=items.find((it)=>it.source_id===process.argv[2]); if(!item) process.exit(3); process.stdout.write(item.id);" "$ITEMS" "$SOURCE_ID")"
JOBS="$(request GET /jobs)"
QUEUED="$(node -e "const jobs=JSON.parse(process.argv[1]); process.stdout.write(String(jobs.filter((it)=>it.item_id===process.argv[2] && it.status==='queued').length));" "$JOBS" "$ITEM_ID")"
if [ "$QUEUED" = "0" ]; then
  echo "file_video source did not queue an indexing job." >&2
  echo "$SOURCE" >&2
  echo "$JOBS" >&2
  exit 1
fi

started_at="$(date +%s)"
while true; do
  ITEMS="$(request GET /items)"
  ITEM_STATUS="$(node -e "const items=JSON.parse(process.argv[1]); const item=items.find((it)=>it.id===process.argv[2]); process.stdout.write(item?.status ?? 'missing');" "$ITEMS" "$ITEM_ID")"
  JOBS="$(request GET /jobs)"
  JOB_STATUS="$(node -e "const jobs=JSON.parse(process.argv[1]); const job=jobs.find((it)=>it.item_id===process.argv[2]); process.stdout.write(job?.status ?? 'missing');" "$JOBS" "$ITEM_ID")"

  if [ "$ITEM_STATUS" = "indexed" ] || [ "$JOB_STATUS" = "completed" ] || [ "$JOB_STATUS" = "succeeded" ]; then
    break
  fi
  if [ "$ITEM_STATUS" = "failed" ] || [ "$JOB_STATUS" = "failed" ]; then
    echo "local model indexing failed item_status=$ITEM_STATUS job_status=$JOB_STATUS" >&2
    echo "$JOBS" >&2
    sed -n '1,240p' "$API_LOG" >&2 || true
    exit 1
  fi
  now="$(date +%s)"
  if [ $((now - started_at)) -ge "$TIMEOUT_SECONDS" ]; then
    echo "timed out waiting for local model indexing item=$ITEM_ID status=$ITEM_STATUS job=$JOB_STATUS" >&2
    echo "$JOBS" >&2
    sed -n '1,240p' "$API_LOG" >&2 || true
    exit 1
  fi
  sleep 5
done

CHUNKS="$(request GET "/items/$ITEM_ID/chunks")"
TRANSCRIPTS="$(node -e "const chunks=JSON.parse(process.argv[1]); process.stdout.write(String(chunks.filter((c)=>c.chunk_type==='transcript').length));" "$CHUNKS")"
TRANSCRIPT_LINES="$(node -e "const chunks=JSON.parse(process.argv[1]); process.stdout.write(String(chunks.filter((c)=>c.chunk_type==='transcript_line').length));" "$CHUNKS")"
KEYFRAMES="$(node -e "const chunks=JSON.parse(process.argv[1]); process.stdout.write(String(chunks.filter((c)=>c.chunk_type==='keyframe').length));" "$CHUNKS")"
PLAYABLE_CHUNK_ID="$(node -e "const chunks=JSON.parse(process.argv[1]); const chunk=chunks.find((c)=>c.chunk_type==='transcript') ?? chunks.find((c)=>c.chunk_type==='keyframe'); if(!chunk) process.exit(3); process.stdout.write(chunk.id);" "$CHUNKS")"

if [ "$TRANSCRIPTS" -le 0 ] || [ "$TRANSCRIPT_LINES" -le 0 ] || [ "$KEYFRAMES" -le 0 ]; then
  echo "Electron video smoke did not produce expected artifacts transcripts=$TRANSCRIPTS transcript_lines=$TRANSCRIPT_LINES keyframes=$KEYFRAMES" >&2
  echo "$CHUNKS" >&2
  exit 1
fi

RANGE_STATUS="$(curl -fsS -o /dev/null -w '%{http_code}' -H 'Range: bytes=0-1023' "http://127.0.0.1:7777/chunks/$PLAYABLE_CHUNK_ID/video-segment")"
if [ "$RANGE_STATUS" != "206" ]; then
  echo "video segment range request did not return 206: status=$RANGE_STATUS chunk=$PLAYABLE_CHUNK_ID" >&2
  exit 1
fi

CERUL_ELECTRON_VIDEO_SMOKE=1 \
CERUL_ELECTRON_VIDEO_SMOKE_ITEM_ID="$ITEM_ID" \
CERUL_ELECTRON_VIDEO_SMOKE_TIMEOUT_MS=90000 \
pnpm --filter @cerul/electron-shell start >"$ELECTRON_LOG" 2>&1 &
ELECTRON_PID=$!

started_at="$(date +%s)"
while kill -0 "$ELECTRON_PID" 2>/dev/null; do
  now="$(date +%s)"
  if [ $((now - started_at)) -ge "$TIMEOUT_SECONDS" ]; then
    echo "timed out waiting for Electron video playback smoke." >&2
    sed -n '1,260p' "$ELECTRON_LOG" >&2 || true
    exit 1
  fi
  sleep 1
done

if ! wait "$ELECTRON_PID"; then
  echo "Electron video playback smoke exited with failure." >&2
  sed -n '1,260p' "$ELECTRON_LOG" >&2 || true
  exit 1
fi

if ! grep -q "electron_video_playback_smoke status=ok" "$ELECTRON_LOG"; then
  echo "Electron video playback smoke did not report success." >&2
  sed -n '1,260p' "$ELECTRON_LOG" >&2 || true
  exit 1
fi

SMOKE_LINE="$(grep "electron_video_playback_smoke status=ok" "$ELECTRON_LOG" | tail -1)"
echo "$SMOKE_LINE range_status=$RANGE_STATUS transcripts=$TRANSCRIPTS transcript_lines=$TRANSCRIPT_LINES keyframes=$KEYFRAMES video=$VIDEO_PATH models_cache=$CERUL_MLX_MODELS_CACHE ffmpeg_path=$CERUL_FFMPEG_PATH"
