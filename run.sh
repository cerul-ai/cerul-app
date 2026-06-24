#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

source scripts/load-env.sh
export GGML_NATIVE="${GGML_NATIVE:-OFF}"

host_target() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-aarch64 | Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "unsupported" ;;
  esac
}

needs_bundled_binaries() {
  local target="$1"
  local suffix=""
  if [[ "$target" == *pc-windows-msvc ]]; then
    suffix=".exe"
  fi
  local dir="$ROOT/third-party/$target"
  local ytdlp_version
  ytdlp_version="$(node -e 'const fs = require("fs"); const manifest = JSON.parse(fs.readFileSync("third-party/yt-dlp-manifest.json", "utf8")); process.stdout.write(String(process.env.CERUL_YTDLP_VERSION || manifest.version));')"
  local ffmpeg_version="${CERUL_FFMPEG_VERSION:-7.1}"
  local qdrant_version="${CERUL_QDRANT_VERSION:-v1.18.2}"

  [ -x "$dir/ffmpeg$suffix" ] || return 0
  [ -x "$dir/yt-dlp$suffix" ] || return 0
  [ -x "$dir/qdrant$suffix" ] || return 0
  [ "$(cat "$dir/.ffmpeg-version" 2>/dev/null || true)" = "$ffmpeg_version" ] || return 0
  [ "$(cat "$dir/.yt-dlp-version" 2>/dev/null || true)" = "$ytdlp_version" ] || return 0
  [ "$(cat "$dir/.qdrant-version" 2>/dev/null || true)" = "$qdrant_version" ] || return 0
  return 1
}

if command -v osascript >/dev/null 2>&1; then
  osascript -e 'quit app "Cerul"' >/dev/null 2>&1 || true
fi

for _ in {1..20}; do
  if ! lsof -nP -iTCP:7777 -sTCP:LISTEN >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if lsof -nP -iTCP:7777 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Port 7777 is still in use after asking Cerul to quit:"
  lsof -nP -iTCP:7777 -sTCP:LISTEN
  echo "Quit the process above, then rerun ./run.sh."
  exit 1
fi

TARGET_TRIPLE="$(host_target)"
if [ "$TARGET_TRIPLE" = "unsupported" ]; then
  echo "Cannot infer target triple for this host; scripts/fetch-binaries.sh will report details."
  CERUL_BINARY_PROBE_TIMEOUT_SEC="${CERUL_BINARY_PROBE_TIMEOUT_SEC:-60}" bash scripts/fetch-binaries.sh
elif needs_bundled_binaries "$TARGET_TRIPLE"; then
  CERUL_BINARY_PROBE_TIMEOUT_SEC="${CERUL_BINARY_PROBE_TIMEOUT_SEC:-60}" bash scripts/fetch-binaries.sh
fi

bash scripts/clean-dev-runtime.sh
pnpm --filter @cerul/electron-shell dev 2> >(grep -v 'representedObject is not a WeakPtrToElectronMenuModelAsNSObject' >&2)
