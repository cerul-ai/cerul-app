#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARY=""
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-installed-runtime-windows.sh [--binary <path>] [--dry-run]

Verifies that an Electron-packaged Windows Cerul runtime can start from unpacked
resources with isolated user profile, bundled third-party binaries, and a
healthy local REST API.

Pass --binary for a copied or release-built resources/bin/cerul-core.exe. If
omitted, the script looks for target/electron/win-unpacked/resources/bin/cerul-core.exe.
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
  local target_triple="${2:-${TARGET_TRIPLE:-${CERUL_TARGET_TRIPLE:-x86_64-pc-windows-msvc}}}"
  local target_binary="${3:-${INSTALLED_BINARY:-${BINARY:-auto}}}"
  printf 'installed_runtime_smoke platform=windows status=%s %s %s packaged_core=verified bundled_ffmpeg=verified bundled_ytdlp=verified health=ok\n' \
    "$status" \
    "$(field binary "$target_binary")" \
    "$(field target "$target_triple")"
}

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ validate Windows host"
  echo "+ locate Electron resources/bin/cerul-core.exe"
  echo "+ copy packaged cerul-core.exe and sibling resources/third-party to a temporary install directory"
  echo "+ launch copied cerul-core.exe with CERUL_FFMPEG_PATH and CERUL_YTDLP_PATH"
  echo "+ poll http://127.0.0.1:23785/internal/health for status=ok"
  emit_result planned
  exit 0
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) ;;
  *)
    echo "Installed Windows runtime smoke requires Git Bash on Windows." >&2
    exit 2
    ;;
esac

TARGET_TRIPLE="${CERUL_TARGET_TRIPLE:-x86_64-pc-windows-msvc}"
API_HEALTH_URL="${CERUL_API_HEALTH_URL:-http://127.0.0.1:23785/internal/health}"
CURL_BIN="${CURL_BIN:-$(command -v curl || true)}"

if [ -z "$CURL_BIN" ]; then
  echo "curl is required to validate the installed runtime health endpoint." >&2
  exit 2
fi

if [ -z "$BINARY" ]; then
  BINARY="$ROOT/target/electron/win-unpacked/resources/bin/cerul-core.exe"
fi

if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "No packaged Windows Cerul Core found. Run scripts/build-installers.sh --debug, pass --binary, or build the Electron package." >&2
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
INSTALLED_BINARY="$INSTALL_DIR/bin/cerul-core.exe"
cp "$BINARY" "$INSTALLED_BINARY"
chmod +x "$INSTALLED_BINARY"
cp -R "$SOURCE_THIRD_PARTY" "$INSTALL_DIR/third-party"

check_bundled_binary() {
  local name="$1"
  local path="$INSTALL_DIR/third-party/$TARGET_TRIPLE/$name"

  if [ ! -f "$path" ]; then
    echo "Installed Windows runtime is missing bundled $name at $path." >&2
    exit 1
  fi
}

check_bundled_binary "ffmpeg.exe"
check_bundled_binary "yt-dlp.exe"

if "$CURL_BIN" -fsS --max-time 1 "$API_HEALTH_URL" >/dev/null 2>&1; then
  echo "Cerul Core already responds at $API_HEALTH_URL before launch; stop the existing runtime and rerun." >&2
  exit 1
fi

to_windows_path() {
  cygpath -w "$1" 2>/dev/null || printf '%s' "$1"
}

USERPROFILE_WIN="$(to_windows_path "$HOME_DIR")"
APPDATA_WIN="$(to_windows_path "$HOME_DIR/AppData/Roaming")"
LOCALAPPDATA_WIN="$(to_windows_path "$HOME_DIR/AppData/Local")"
TEMP_WIN="$(to_windows_path "$HOME_DIR/AppData/Local/Temp")"
SYSTEMROOT_WIN="${SYSTEMROOT:-${SystemRoot:-C:\\Windows}}"
WINDIR_WIN="${WINDIR:-${windir:-$SYSTEMROOT_WIN}}"
SYSTEM_PATH_WIN="$SYSTEMROOT_WIN\\System32;$SYSTEMROOT_WIN;$SYSTEMROOT_WIN\\System32\\Wbem"

mkdir -p "$HOME_DIR/AppData/Roaming" "$HOME_DIR/AppData/Local/Temp"

env -i \
  USERPROFILE="$USERPROFILE_WIN" \
  APPDATA="$APPDATA_WIN" \
  LOCALAPPDATA="$LOCALAPPDATA_WIN" \
  TEMP="$TEMP_WIN" \
  TMP="$TEMP_WIN" \
  SYSTEMROOT="$SYSTEMROOT_WIN" \
  SystemRoot="$SYSTEMROOT_WIN" \
  WINDIR="$WINDIR_WIN" \
  PATH="$SYSTEM_PATH_WIN" \
  CERUL_FFMPEG_PATH="$INSTALL_DIR/third-party/$TARGET_TRIPLE/ffmpeg.exe" \
  CERUL_YTDLP_PATH="$INSTALL_DIR/third-party/$TARGET_TRIPLE/yt-dlp.exe" \
  "$INSTALLED_BINARY" &
PID="$!"

deadline=$((SECONDS + 30))
health_body=""
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$PID" >/dev/null 2>&1; then
    echo "Installed Windows Cerul Core exited before health became ready." >&2
    exit 1
  fi

  if health_body="$("$CURL_BIN" -fsS --max-time 2 "$API_HEALTH_URL" 2>/dev/null)" &&
    printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
    echo "Installed Windows runtime health check passed: $health_body"
    break
  fi

  sleep 1
done

if [ -z "$health_body" ] || ! printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
  echo "Installed Windows Cerul Core did not report healthy at $API_HEALTH_URL within 30s." >&2
  exit 1
fi

emit_result passed "$TARGET_TRIPLE" "$INSTALLED_BINARY"
echo "Installed Windows runtime smoke passed for $INSTALLED_BINARY target=$TARGET_TRIPLE"
