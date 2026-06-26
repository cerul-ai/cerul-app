#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

source scripts/load-env.sh
export GGML_NATIVE="${GGML_NATIVE:-OFF}"

saved_api_port() {
  node <<'NODE'
const fs = require("fs");
const os = require("os");
const path = require("path");

function dataBaseDir() {
  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Application Support");
  }
  if (process.platform === "win32") {
    return process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming");
  }
  return process.env.XDG_DATA_HOME || path.join(os.homedir(), ".local", "share");
}

function parsePort(value) {
  const port = Number.parseInt(String(value || ""), 10);
  return Number.isInteger(port) && port >= 1024 && port <= 65535 ? port : null;
}

const dataDir = process.env.CERUL_DATA_DIR || path.join(dataBaseDir(), "Cerul");
try {
  const endpoint = JSON.parse(fs.readFileSync(path.join(dataDir, "endpoint.json"), "utf8"));
  const port = parsePort(endpoint.port)
    || (typeof endpoint.base_url === "string" ? parsePort(new URL(endpoint.base_url).port) : null);
  if (port) {
    process.stdout.write(String(port));
  }
} catch {
}
NODE
}

if [ -n "${CERUL_API_PORT:-}" ]; then
  API_PORT="$CERUL_API_PORT"
  export CERUL_API_PORT
else
  API_PORT="$(saved_api_port)"
  API_PORT="${API_PORT:-23785}"
  export CERUL_API_PORT="$API_PORT"
fi

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

can_auto_stage_bundled_binaries() {
  local target="$1"
  case "$target" in
    *apple-darwin)
      return 0
      ;;
  esac
  local target_env
  target_env="CERUL_FFMPEG_URL_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')"
  if [ -n "${CERUL_FFMPEG_URL:-}" ] || [ -n "${!target_env:-}" ]; then
    return 0
  fi
  if command -v ffmpeg >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

if command -v osascript >/dev/null 2>&1; then
  osascript -e 'quit app "Cerul"' >/dev/null 2>&1 || true
fi

for _ in {1..20}; do
  if ! lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Port $API_PORT is still in use after asking Cerul to quit:"
  lsof -nP -iTCP:"$API_PORT" -sTCP:LISTEN
  echo "Quit the process above, then rerun ./run.sh."
  exit 1
fi

TARGET_TRIPLE="$(host_target)"
if [ "$TARGET_TRIPLE" = "unsupported" ]; then
  echo "Cannot infer target triple for this host; scripts/fetch-binaries.sh will report details."
  CERUL_BINARY_PROBE_TIMEOUT_SEC="${CERUL_BINARY_PROBE_TIMEOUT_SEC:-60}" bash scripts/fetch-binaries.sh
elif needs_bundled_binaries "$TARGET_TRIPLE"; then
  if ! can_auto_stage_bundled_binaries "$TARGET_TRIPLE"; then
    echo "Skipping bundled binary staging for $TARGET_TRIPLE: no default ffmpeg download is configured and ffmpeg is not on PATH."
    echo "Install ffmpeg or set CERUL_FFMPEG_URL to enable bundled media tooling for web video imports."
  else
    CERUL_BINARY_PROBE_TIMEOUT_SEC="${CERUL_BINARY_PROBE_TIMEOUT_SEC:-60}" bash scripts/fetch-binaries.sh
  fi
fi

bash scripts/clean-dev-runtime.sh
pnpm --filter @cerul/electron-shell dev 2> >(grep -v 'representedObject is not a WeakPtrToElectronMenuModelAsNSObject' >&2)
