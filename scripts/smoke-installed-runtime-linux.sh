#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARY=""
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-installed-runtime-linux.sh [--binary <path>] [--dry-run]

Verifies that an Electron-packaged Linux Cerul runtime can start from unpacked
resources with isolated HOME, bundled third-party binaries, and a healthy local
REST API.

Pass --binary for a copied or release-built resources/bin/cerul-core executable.
If omitted, the script looks for target/electron/linux-unpacked/resources/bin/cerul-core.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --binary)
      BINARY="${2:?missing binary path}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
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

field() {
  local key="$1"
  local value="$2"
  printf '%s=%q' "$key" "$value"
}

emit_result() {
  local status="$1"
  local target_triple="${2:-${TARGET_TRIPLE:-${CERUL_TARGET_TRIPLE:-auto}}}"
  local target_binary="${3:-${INSTALLED_BINARY:-${BINARY:-auto}}}"
  printf 'installed_runtime_smoke platform=linux status=%s %s %s packaged_core=verified bundled_ffmpeg=verified bundled_ytdlp=verified bundled_qdrant=verified health=ok\n' \
    "$status" \
    "$(field binary "$target_binary")" \
    "$(field target "$target_triple")"
}

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ validate Linux host"
  echo "+ locate Electron resources/bin/cerul-core"
  echo "+ copy packaged cerul-core and sibling resources/third-party to a temporary install directory"
  echo "+ launch copied cerul-core with CERUL_FFMPEG_PATH, CERUL_YTDLP_PATH, and CERUL_QDRANT_BIN"
  echo "+ poll http://127.0.0.1:23785/internal/health for status=ok"
  emit_result planned
  exit 0
fi

if [ "$(uname -s)" != "Linux" ]; then
  echo "Installed Linux runtime smoke requires Linux." >&2
  exit 2
fi

host_target_triple() {
  case "$(uname -m)" in
    aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
    x86_64) echo "x86_64-unknown-linux-gnu" ;;
    *)
      echo "Unsupported Linux architecture for bundled binary smoke: $(uname -m)" >&2
      exit 2
      ;;
  esac
}

TARGET_TRIPLE="${CERUL_TARGET_TRIPLE:-$(host_target_triple)}"
API_HEALTH_URL="${CERUL_API_HEALTH_URL:-http://127.0.0.1:23785/internal/health}"
CURL_BIN="${CURL_BIN:-$(command -v curl || true)}"

if [ -z "$CURL_BIN" ] || [ ! -x "$CURL_BIN" ]; then
  echo "curl is required to validate the installed runtime health endpoint." >&2
  exit 2
fi

if [ -z "$BINARY" ]; then
  BINARY="$ROOT/target/electron/linux-unpacked/resources/bin/cerul-core"
fi

if [ -z "$BINARY" ] || [ ! -x "$BINARY" ]; then
  echo "No executable packaged Cerul Core found. Run scripts/build-installers.sh --debug, pass --binary, or build the Electron package." >&2
  exit 1
fi

SOURCE_RESOURCES="$(cd "$(dirname "$BINARY")/.." && pwd)"
SOURCE_THIRD_PARTY="$SOURCE_RESOURCES/third-party"
if [ ! -d "$SOURCE_THIRD_PARTY" ]; then
  echo "Packaged runtime is missing sibling third-party resources: $SOURCE_THIRD_PARTY" >&2
  exit 1
fi

HOME_DIR="$(mktemp -d)"
INSTALL_DIR="$(mktemp -d)"
PID=""

cleanup() {
  if [ -n "$PID" ]; then
    kill "$PID" >/dev/null 2>&1 || true
    wait "$PID" >/dev/null 2>&1 || true
  fi
  rm -rf "$HOME_DIR" "$INSTALL_DIR" || true
}
trap cleanup EXIT

mkdir -p "$INSTALL_DIR/bin"
INSTALLED_BINARY="$INSTALL_DIR/bin/cerul-core"
cp "$BINARY" "$INSTALLED_BINARY"
chmod +x "$INSTALLED_BINARY"
cp -R "$SOURCE_THIRD_PARTY" "$INSTALL_DIR/third-party"

check_bundled_binary() {
  local name="$1"
  local path="$INSTALL_DIR/third-party/$TARGET_TRIPLE/$name"

  if [ ! -x "$path" ]; then
    echo "Installed Linux runtime is missing executable bundled $name at $path." >&2
    exit 1
  fi
}

check_bundled_binary "ffmpeg"
check_bundled_binary "yt-dlp"
check_bundled_binary "qdrant"

if "$CURL_BIN" -fsS --max-time 1 "$API_HEALTH_URL" >/dev/null 2>&1; then
  echo "Cerul Core already responds at $API_HEALTH_URL before launch; stop the existing runtime and rerun." >&2
  exit 1
fi

env -i \
  HOME="$HOME_DIR" \
  XDG_RUNTIME_DIR="$HOME_DIR/xdg-runtime" \
  PATH="/usr/bin:/bin" \
  CERUL_FFMPEG_PATH="$INSTALL_DIR/third-party/$TARGET_TRIPLE/ffmpeg" \
  CERUL_YTDLP_PATH="$INSTALL_DIR/third-party/$TARGET_TRIPLE/yt-dlp" \
  CERUL_QDRANT_BIN="$INSTALL_DIR/third-party/$TARGET_TRIPLE/qdrant" \
  "$INSTALLED_BINARY" &
PID="$!"

deadline=$((SECONDS + 30))
health_body=""
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$PID" >/dev/null 2>&1; then
    echo "Installed Linux Cerul Core exited before health became ready." >&2
    exit 1
  fi

  if health_body="$("$CURL_BIN" -fsS --max-time 2 "$API_HEALTH_URL" 2>/dev/null)" &&
    printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
    echo "Installed Linux runtime health check passed: $health_body"
    break
  fi

  sleep 1
done

if [ -z "$health_body" ] || ! printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
  echo "Installed Linux Cerul Core did not report healthy at $API_HEALTH_URL within 30s." >&2
  exit 1
fi

emit_result passed "$TARGET_TRIPLE" "$INSTALLED_BINARY"
echo "Installed Linux runtime smoke passed for $INSTALLED_BINARY target=$TARGET_TRIPLE"
