#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG=""
DRY_RUN=0
DEFAULT_APP_VERSION="0.0.3"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dmg)
      if [ "$#" -lt 2 ]; then
        echo "--dmg requires a path." >&2
        exit 2
      fi
      DMG="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      echo "usage: scripts/smoke-installed-app.sh [--dmg <path>|<path>] [--dry-run]" >&2
      exit 0
      ;;
    *)
      if [ -n "$DMG" ]; then
        echo "Unexpected argument: $1" >&2
        exit 2
      fi
      DMG="$1"
      shift
      ;;
  esac
done

if [ "$(uname -s)" != "Darwin" ] && [ "$DRY_RUN" -eq 0 ]; then
  echo "Installed-app smoke currently requires macOS because it mounts a DMG." >&2
  exit 2
fi

host_target_triple() {
  case "$(uname -m)" in
    arm64|aarch64)
      echo "aarch64-apple-darwin"
      ;;
    x86_64)
      echo "x86_64-apple-darwin"
      ;;
    *)
      echo "Unsupported macOS architecture for bundled binary smoke: $(uname -m)" >&2
      exit 2
      ;;
  esac
}

TARGET_TRIPLE="${CERUL_TARGET_TRIPLE:-$(host_target_triple)}"
API_BASE_URL="${CERUL_API_BASE_URL:-http://127.0.0.1:7777}"
API_HEALTH_URL="${CERUL_API_HEALTH_URL:-$API_BASE_URL/health}"
CURL_BIN="${CURL_BIN:-/usr/bin/curl}"

if command -v node >/dev/null 2>&1 && [ -f "$ROOT/apps/electron-shell/package.json" ]; then
  DEFAULT_APP_VERSION="$(cd "$ROOT" && node -p "require('./apps/electron-shell/package.json').version")"
fi

if [ ! -x "$CURL_BIN" ]; then
  if command -v curl >/dev/null 2>&1; then
    CURL_BIN="$(command -v curl)"
  else
    echo "curl is required to validate the installed runtime health endpoint." >&2
    exit 2
  fi
fi

if [ -z "$DMG" ]; then
  if [ -d "$ROOT/target/electron" ]; then
    DMG="$(find "$ROOT/target/electron" -maxdepth 2 -name "*.dmg" -type f -print 2>/dev/null | sort | tail -1)"
  fi
fi

if [ -z "$DMG" ] || [ ! -f "$DMG" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    DMG="${DMG:-target/electron/Cerul-$DEFAULT_APP_VERSION-arm64.dmg}"
  else
    echo "No DMG found. Run scripts/build-installers.sh first or pass a DMG path." >&2
    exit 1
  fi
fi

field() {
  local key="$1"
  local value="$2"
  printf '%s=%q' "$key" "$value"
}

emit_result() {
  local status="$1"
  printf 'installed_app_smoke status=%s %s %s bundled_ffmpeg=verified bundled_ytdlp=verified settings=roundtrip inference_mode=remote folder_source=queued health=ok\n' \
    "$status" \
    "$(field dmg "$DMG")" \
    "$(field target "$TARGET_TRIPLE")"
}

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ mount $DMG and copy Cerul.app to a temporary install directory"
  echo "+ verify bundled third-party/$TARGET_TRIPLE/ffmpeg and yt-dlp are present and runnable"
  echo "+ verify bundled macOS ffmpeg dependencies use app-relative loader paths"
  echo "+ verify Contents/Resources/bin/cerul-api is executable"
  echo "+ launch installed Electron app with isolated HOME and stripped PATH"
  echo "+ poll $API_HEALTH_URL for installed runtime health"
  echo "+ roundtrip settings and inference mode through $API_BASE_URL"
  echo "+ add temporary folder_video source and verify discovered item plus queued job"
  emit_result planned
  exit 0
fi

MOUNT_DIR="$(mktemp -d)"
HOME_DIR="$(mktemp -d)"
INSTALL_DIR="$(mktemp -d)"
PID=""
ATTACH_LOG=""

cleanup() {
  if [ -n "$PID" ]; then
    kill "$PID" >/dev/null 2>&1 || true
    wait "$PID" >/dev/null 2>&1 || true
  fi
  hdiutil detach "$MOUNT_DIR" -quiet >/dev/null 2>&1 || \
    hdiutil detach "$MOUNT_DIR" -force -quiet >/dev/null 2>&1 || true
  rm -rf "$HOME_DIR" "$INSTALL_DIR" "$ATTACH_LOG" || true
  rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

ATTACH_LOG="$(mktemp)"
if ! printf 'Y\n' | hdiutil attach "$DMG" -mountpoint "$MOUNT_DIR" -nobrowse -readonly >"$ATTACH_LOG" 2>&1; then
  cat "$ATTACH_LOG" >&2
  exit 1
fi

APP_PATH="$(find "$MOUNT_DIR" -maxdepth 2 -name "Cerul.app" -type d -print -quit)"
if [ -z "$APP_PATH" ]; then
  echo "Mounted DMG did not contain Cerul.app." >&2
  exit 1
fi

INSTALLED_APP_PATH="$INSTALL_DIR/Cerul.app"
if ! ditto "$APP_PATH" "$INSTALLED_APP_PATH"; then
  echo "Failed to copy Cerul.app from mounted DMG to temporary install directory." >&2
  exit 1
fi

APP_PATH="$INSTALLED_APP_PATH"
BIN_PATH="$(find "$APP_PATH/Contents/MacOS" -maxdepth 1 -type f -print -quit)"
if [ -z "$BIN_PATH" ] || [ ! -x "$BIN_PATH" ]; then
  echo "Cerul.app does not contain an executable in Contents/MacOS." >&2
  exit 1
fi

if [ ! -d "$APP_PATH/Contents/Resources/third-party" ]; then
  echo "Cerul.app does not contain bundled third-party resources." >&2
  exit 1
fi

API_BIN="$APP_PATH/Contents/Resources/bin/cerul-api"
if [ ! -x "$API_BIN" ]; then
  echo "Cerul.app does not contain executable packaged cerul-api: $API_BIN" >&2
  exit 1
fi

check_bundled_binary() {
  local name="$1"
  shift
  local path="$APP_PATH/Contents/Resources/third-party/$TARGET_TRIPLE/$name"

  if [ ! -f "$path" ]; then
    echo "Cerul.app is missing bundled $name at $path." >&2
    exit 1
  fi

  if [ ! -x "$path" ]; then
    echo "Bundled $name is not executable: $path." >&2
    exit 1
  fi

  if [ "$#" -gt 0 ] && ! run_bundled_binary "$path" "$@"; then
    echo "Bundled $name is not runnable: $path" >&2
    exit 1
  fi
}

run_bundled_binary() {
  local path="$1"
  shift
  local timeout_sec="${CERUL_BUNDLED_BINARY_TIMEOUT_SEC:-60}"

  "$path" "$@" >/dev/null 2>&1 &
  local pid="$!"
  local i
  for ((i = 1; i <= timeout_sec; i++)); do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      wait "$pid"
      return $?
    fi
    sleep 1
  done

  kill -9 "$pid" >/dev/null 2>&1 || true
  return 124
}

check_macos_loader_deps() {
  local path="$1"
  local loader_dir
  loader_dir="$(cd "$(dirname "$path")" && pwd)"
  local dep dep_path

  while IFS= read -r dep; do
    case "$dep" in
      /usr/lib/*|/System/Library/*)
        ;;
      @loader_path/*)
        dep_path="$loader_dir/${dep#@loader_path/}"
        if [ ! -f "$dep_path" ]; then
          echo "Bundled binary dependency is missing: $path -> $dep ($dep_path)" >&2
          exit 1
        fi
        ;;
      *)
        echo "Bundled binary has an unbundled macOS dependency: $path -> $dep" >&2
        exit 1
        ;;
    esac
  done < <(otool -L "$path" 2>/dev/null | awk 'NR > 1 { print $1 }')
}

FFMPEG_PATH="$APP_PATH/Contents/Resources/third-party/$TARGET_TRIPLE/ffmpeg"
if [ "$(uname -s)" = "Darwin" ]; then
  check_macos_loader_deps "$FFMPEG_PATH"
fi
check_bundled_binary "ffmpeg" -version
check_bundled_binary "yt-dlp" --version

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  printf '%s' "$value"
}

api_request() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  local url="$API_BASE_URL$path"

  if [ "$method" = "GET" ]; then
    "$CURL_BIN" -fsS --max-time 5 "$url"
  else
    "$CURL_BIN" -fsS --max-time 5 \
      -X "$method" \
      -H "Content-Type: application/json" \
      --data "$body" \
      "$url"
  fi
}

require_json_field() {
  local body="$1"
  local field="$2"
  local expected="$3"
  local label="$4"

  if ! printf '%s' "$body" |
    grep -Eq "\"$field\"[[:space:]]*:[[:space:]]*$expected"; then
    echo "$label did not contain expected JSON field $field=$expected:" >&2
    printf '%s\n' "$body" >&2
    exit 1
  fi
}

require_json_field_absent() {
  local body="$1"
  local field="$2"
  local label="$3"

  if printf '%s' "$body" | grep -Eq "\"$field\"[[:space:]]*:"; then
    echo "$label unexpectedly contained JSON field $field:" >&2
    printf '%s\n' "$body" >&2
    exit 1
  fi
}

if "$CURL_BIN" -fsS --max-time 1 "$API_HEALTH_URL" >/dev/null 2>&1; then
  echo "Cerul API already responds at $API_HEALTH_URL before launch; stop the existing runtime and rerun." >&2
  exit 1
fi

env -i HOME="$HOME_DIR" PATH="/usr/bin:/bin" "$BIN_PATH" &
PID="$!"

deadline=$((SECONDS + 30))
health_body=""
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$PID" >/dev/null 2>&1; then
    echo "Installed Cerul app exited before health became ready." >&2
    exit 1
  fi

  if health_body="$("$CURL_BIN" -fsS --max-time 2 "$API_HEALTH_URL" 2>/dev/null)" &&
    printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
    echo "Installed runtime health check passed: $health_body"
    break
  fi

  sleep 1
done

if [ -z "$health_body" ] || ! printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
  echo "Installed Cerul app did not report healthy at $API_HEALTH_URL within 30s." >&2
  exit 1
fi

settings_body="$(api_request PATCH /settings '{"close_to_tray":true,"indexing_paused":true,"inference_mode":"remote"}')"
require_json_field "$settings_body" "close_to_tray" "true" "Settings update"
require_json_field "$settings_body" "indexing_paused" "true" "Settings update"
require_json_field "$settings_body" "inference_mode" '"remote"' "Settings update"

settings_body="$(api_request GET /settings)"
require_json_field "$settings_body" "close_to_tray" "true" "Settings readback"
require_json_field "$settings_body" "indexing_paused" "true" "Settings readback"
require_json_field "$settings_body" "inference_mode" '"remote"' "Settings readback"
echo "Installed runtime settings roundtrip passed."

MEDIA_DIR="$HOME_DIR/installed-smoke-media"
mkdir -p "$MEDIA_DIR"
printf 'cerul installed app smoke\n' >"$MEDIA_DIR/installed-smoke.mp4"

escaped_media_dir="$(json_escape "$MEDIA_DIR")"
source_body="$(api_request POST /sources "{\"type\":\"folder_video\",\"config\":{\"path\":\"$escaped_media_dir\"}}")"
require_json_field "$source_body" "type" '"folder_video"' "Folder source add"
require_json_field "$source_body" "status" '"active"' "Folder source add"

items_body="$(api_request GET /items)"
require_json_field "$items_body" "title" '"installed-smoke"' "Installed folder discovery"
require_json_field "$items_body" "content_type" '"video"' "Installed folder discovery"
require_json_field "$items_body" "status" '"discovered"' "Installed folder discovery"

jobs_body="$(api_request GET /jobs)"
require_json_field "$jobs_body" "job_type" '"index_video"' "Installed folder queue"
require_json_field "$jobs_body" "status" '"queued"' "Installed folder queue"
echo "Installed runtime folder source discovery and queueing passed."

emit_result passed
echo "Installed-app smoke passed for $DMG via $INSTALLED_APP_PATH"
