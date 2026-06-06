#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
LIVE=0
FETCH_FIRST=0
INDEX_FIRST=0
CHANNEL_URL="https://www.youtube.com/@karpathy"
MAX_VIDEOS=5
INDEX_COUNT=1
CLIP_DURATION_SEC=0
YTDLP_PATH=""
TIMEOUT_SECONDS="${CERUL_YOUTUBE_SMOKE_TIMEOUT:-60}"
INDEX_QUERY="cerul youtube indexing smoke phrase"

usage() {
  cat <<'EOF'
Usage: scripts/smoke-youtube-source.sh [--live] [--fetch-first] [--index-first] [--index-count <n>] [--clip-duration-sec <n>] [--channel <url>] [--max <n>] [--ytdlp-path <path>] [--timeout <seconds>]

Default mode uses a fake yt-dlp executable so CI can prove discovery, queueing,
and fetch wiring deterministically. --live uses a real yt-dlp executable against
the public channel URL. --fetch-first is opt-in in live mode because it downloads
the first discovered video. --index-first runs fetched videos through the
indexing/search pipeline with smoke model adapters. --index-count controls how
many of the discovered videos must index and search successfully. The timeout
protects release gates from hanging on network or YouTube-side stalls.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --live)
      LIVE=1
      shift
      ;;
    --fetch-first)
      FETCH_FIRST=1
      shift
      ;;
    --index-first)
      INDEX_FIRST=1
      FETCH_FIRST=1
      shift
      ;;
    --index-count)
      INDEX_FIRST=1
      FETCH_FIRST=1
      INDEX_COUNT="${2:?missing index count}"
      shift 2
      ;;
    --clip-duration-sec)
      CLIP_DURATION_SEC="${2:?missing clip duration seconds}"
      shift 2
      ;;
    --channel)
      CHANNEL_URL="${2:?missing channel URL}"
      shift 2
      ;;
    --max)
      MAX_VIDEOS="${2:?missing max video count}"
      shift 2
      ;;
    --ytdlp-path)
      YTDLP_PATH="${2:?missing yt-dlp path}"
      shift 2
      ;;
    --timeout)
      TIMEOUT_SECONDS="${2:?missing timeout seconds}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$MAX_VIDEOS" in
  ''|*[!0-9]*)
    echo "--max must be a positive integer." >&2
    exit 2
    ;;
esac

if [ "$MAX_VIDEOS" -lt 1 ]; then
  echo "--max must be greater than zero." >&2
  exit 2
fi

case "$INDEX_COUNT" in
  ''|*[!0-9]*)
    echo "--index-count must be a positive integer." >&2
    exit 2
    ;;
esac

if [ "$INDEX_COUNT" -lt 1 ]; then
  echo "--index-count must be greater than zero." >&2
  exit 2
fi

if [ "$INDEX_COUNT" -gt "$MAX_VIDEOS" ]; then
  echo "--index-count cannot be greater than --max." >&2
  exit 2
fi

case "$CLIP_DURATION_SEC" in
  ''|*[!0-9]*)
    echo "--clip-duration-sec must be zero or a positive integer." >&2
    exit 2
    ;;
esac

case "$TIMEOUT_SECONDS" in
  ''|*[!0-9]*)
    echo "--timeout must be a positive integer." >&2
    exit 2
    ;;
esac

if [ "$TIMEOUT_SECONDS" -lt 1 ]; then
  echo "--timeout must be greater than zero." >&2
  exit 2
fi

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

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

if [ "$LIVE" -eq 0 ]; then
  YTDLP_PATH="$TMP_DIR/yt-dlp"
  cat >"$YTDLP_PATH" <<'EOF'
#!/bin/sh
if printf '%s\n' "$@" | grep -q -- '--flat-playlist'; then
  limit=5
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--playlist-end" ]; then
      shift
      limit="$1"
    fi
    shift
  done
  n=1
  while [ "$n" -le "$limit" ]; do
    printf '{"id":"karpathy-%03d","title":"Karpathy sample %d","duration":%d}\n' "$n" "$n" "$((600 + n))"
    n=$((n + 1))
  done
else
  out=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
      shift
      out="$1"
    fi
    shift
  done
  mkdir -p "$(dirname "$out")"
  if [ "${CERUL_FAKE_YTDLP_VIDEO:-0}" = "1" ]; then
    if ! command -v ffmpeg >/dev/null 2>&1; then
      echo "ffmpeg is required for fake index-first YouTube smoke." >&2
      exit 1
    fi
    ffmpeg -hide_banner -loglevel error -y \
      -f lavfi -i testsrc=duration=8:size=64x64:rate=10 \
      -f lavfi -i sine=frequency=880:duration=8 \
      -shortest -c:v mpeg4 -c:a aac -pix_fmt yuv420p "$out"
  else
    printf 'video' > "$out"
  fi
fi
EOF
  chmod +x "$YTDLP_PATH"
  FETCH_FIRST=1
elif [ -z "$YTDLP_PATH" ]; then
  host_triple="$(host_target_triple)"
  if [ "$host_triple" != "unsupported" ] && [ -x "$ROOT/third-party/$host_triple/yt-dlp" ]; then
    YTDLP_PATH="$ROOT/third-party/$host_triple/yt-dlp"
  elif command -v yt-dlp >/dev/null 2>&1; then
    YTDLP_PATH="$(command -v yt-dlp)"
  fi
fi

if [ -z "$YTDLP_PATH" ] || [ ! -x "$YTDLP_PATH" ]; then
  echo "yt-dlp is required for live YouTube smoke; pass --ytdlp-path or run scripts/fetch-binaries.sh." >&2
  exit 2
fi

if [ "$INDEX_FIRST" -eq 1 ]; then
  export CERUL_FAKE_YTDLP_VIDEO=1
  if [ -z "${CERUL_FFMPEG_PATH:-}" ]; then
    if command -v ffmpeg >/dev/null 2>&1; then
      export CERUL_FFMPEG_PATH="$(command -v ffmpeg)"
    else
      echo "ffmpeg is required for index-first YouTube smoke." >&2
      exit 2
    fi
  fi
fi

cd "$ROOT"

list_output="$(
  cargo run -q -p cerul-cli -- \
    list-source youtube \
    --url "$CHANNEL_URL" \
    --max "$MAX_VIDEOS" \
    --ytdlp-path "$YTDLP_PATH" \
    --timeout "$TIMEOUT_SECONDS" \
    --cache-dir "$TMP_DIR/cache"
)"
discovered_count="$(printf '%s\n' "$list_output" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
if [ "$discovered_count" -ne "$MAX_VIDEOS" ]; then
  echo "Expected $MAX_VIDEOS discovered YouTube videos, got $discovered_count." >&2
  printf '%s\n' "$list_output" >&2
  exit 1
fi

add_output="$(
  cargo run -q -p cerul-cli -- \
    --data-dir "$TMP_DIR/data" \
    add-source youtube \
    --url "$CHANNEL_URL" \
    --max "$MAX_VIDEOS" \
    --ytdlp-path "$YTDLP_PATH" \
    --timeout "$TIMEOUT_SECONDS" \
    --cache-dir "$TMP_DIR/cache"
)"
if ! printf '%s\n' "$add_output" | grep -q $'^source\t.*\tyoutube\tactive$'; then
  echo "YouTube source was not persisted as active." >&2
  printf '%s\n' "$add_output" >&2
  exit 1
fi
if ! printf '%s\n' "$add_output" | grep -q "^jobs	$MAX_VIDEOS$"; then
  echo "Expected $MAX_VIDEOS queued YouTube index jobs." >&2
  printf '%s\n' "$add_output" >&2
  exit 1
fi

if [ "$FETCH_FIRST" -eq 1 ]; then
  fetch_args=(
    fetch-first youtube
    --url "$CHANNEL_URL"
    --max "$MAX_VIDEOS"
    --ytdlp-path "$YTDLP_PATH"
    --timeout "$TIMEOUT_SECONDS"
    --cache-dir "$TMP_DIR/cache"
  )
  if [ "$CLIP_DURATION_SEC" -gt 0 ]; then
    fetch_args+=(--clip-duration-sec "$CLIP_DURATION_SEC")
  fi
  fetch_output="$(
    cargo run -q -p cerul-cli -- "${fetch_args[@]}"
  )"
  fetched_bytes="$(printf '%s\n' "$fetch_output" | awk -F '\t' '/^fetched\t/ { print $4; exit }')"
  case "$fetched_bytes" in
    ''|*[!0-9]*)
      fetched_bytes=0
      ;;
  esac
  if [ "$fetched_bytes" -le 0 ]; then
    echo "Expected fetch-first to write a non-empty video file." >&2
    printf '%s\n' "$fetch_output" >&2
    exit 1
  fi
fi

indexed_chunks=0
indexed_frames=0
indexed_items=0
if [ "$INDEX_FIRST" -eq 1 ]; then
  index_args=(
    --data-dir "$TMP_DIR/index-data"
    index-first youtube
    --url "$CHANNEL_URL"
    --max "$MAX_VIDEOS"
    --ytdlp-path "$YTDLP_PATH"
    --timeout "$TIMEOUT_SECONDS"
    --cache-dir "$TMP_DIR/cache"
    --query "$INDEX_QUERY"
    --count "$INDEX_COUNT"
  )
  if [ "$CLIP_DURATION_SEC" -gt 0 ]; then
    index_args+=(--clip-duration-sec "$CLIP_DURATION_SEC")
  fi
  index_output="$(
    cargo run -q -p cerul-cli -- "${index_args[@]}"
  )"
  indexed_items="$(printf '%s\n' "$index_output" | awk -F '\t' '/^indexed\t/ { count++ } END { print count + 0 }')"
  if [ "$indexed_items" -ne "$INDEX_COUNT" ]; then
    echo "Expected index-first to report an indexed YouTube result." >&2
    printf '%s\n' "$index_output" >&2
    exit 1
  fi
  indexed_chunks="$(printf '%s\n' "$index_output" | awk -F '\t' '/^indexed_summary\t/ { print $3; exit }')"
  indexed_frames="$(printf '%s\n' "$index_output" | awk -F '\t' '/^indexed_summary\t/ { print $4; exit }')"
fi

mode="fake"
if [ "$LIVE" -eq 1 ]; then
  mode="live"
fi

echo "youtube_source_smoke mode=$mode channel=$CHANNEL_URL discovered=$discovered_count queued_jobs=$MAX_VIDEOS fetch_first=$FETCH_FIRST index_first=$INDEX_FIRST indexed_items=$indexed_items indexed_chunks=$indexed_chunks indexed_frames=$indexed_frames clip_duration_sec=$CLIP_DURATION_SEC timeout_sec=$TIMEOUT_SECONDS"
