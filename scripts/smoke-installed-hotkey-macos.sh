#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG=""
APP_PATH=""
HOTKEY="${CERUL_HOTKEY_SMOKE_HOTKEY:-Alt+Space}"
TIMEOUT="${CERUL_HOTKEY_SMOKE_TIMEOUT:-15}"
MANUAL_TRIGGER="${CERUL_HOTKEY_SMOKE_MANUAL:-0}"
DRY_RUN=0
API_HEALTH_URL="${CERUL_API_HEALTH_URL:-http://127.0.0.1:23785/internal/health}"
CURL_BIN="${CURL_BIN:-/usr/bin/curl}"

usage() {
  cat <<'EOF'
Usage: scripts/smoke-installed-hotkey-macos.sh [--dmg <path> | --app <Cerul.app>] [--hotkey <label>] [--timeout <seconds>] [--manual] [--dry-run]

Launches an installed macOS Cerul.app, sends the configured
global hotkey through System Events, verifies a visible Cerul window appears,
then sends Escape and verifies it hides again.

The --hotkey label is applied through CERUL_GLOBAL_HOTKEY before launch so the
installed app registers the same shortcut that this smoke sends.

Use --manual when synthetic System Events keypresses do not reach the macOS
global shortcut handler. In that mode, the script waits for a physical keypress
and still verifies the installed app window appears and hides again.

Supported hotkeys: Alt+Space, Ctrl+Space, Ctrl+Shift+Space, Cmd+Shift+Space.

This smoke requires macOS Accessibility permission for the terminal/Codex
runner that executes the script. If System Events cannot send keystrokes or
inspect windows, grant the runner in System Settings > Privacy & Security >
Accessibility and rerun.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dmg)
      DMG="${2:?missing DMG path}"
      shift 2
      ;;
    --app)
      APP_PATH="${2:?missing Cerul.app path}"
      shift 2
      ;;
    --hotkey)
      HOTKEY="${2:?missing hotkey label}"
      shift 2
      ;;
    --timeout)
      TIMEOUT="${2:?missing timeout seconds}"
      shift 2
      ;;
    --manual)
      MANUAL_TRIGGER=1
      shift
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

case "$HOTKEY" in
  Alt+Space|Ctrl+Space|Ctrl+Shift+Space|Cmd+Shift+Space)
    ;;
  *)
    echo "Unsupported hotkey: $HOTKEY" >&2
    usage >&2
    exit 2
    ;;
esac

if ! [[ "$TIMEOUT" =~ ^[0-9]+$ ]] || [ "$TIMEOUT" -lt 1 ]; then
  echo "--timeout must be a positive integer." >&2
  exit 2
fi

case "$MANUAL_TRIGGER" in
  0|1)
    ;;
  *)
    echo "CERUL_HOTKEY_SMOKE_MANUAL must be 0 or 1." >&2
    exit 2
    ;;
esac

if [ -n "$DMG" ] && [ -n "$APP_PATH" ]; then
  echo "Pass either --dmg or --app, not both." >&2
  exit 2
fi

field() {
  local key="$1"
  local value="$2"
  printf '%s=%q' "$key" "$value"
}

emit_result() {
  local status="$1"
  local target="${APP_PATH:-${DMG:-auto}}"
  local trigger="synthetic"
  if [ "$MANUAL_TRIGGER" -eq 1 ]; then
    trigger="manual"
  fi

  printf 'installed_hotkey_smoke status=%s %s %s trigger=%s timeout=%ss %s overlay=shown_and_hidden health=ok\n' \
    "$status" \
    "$(field target "$target")" \
    "$(field hotkey "$HOTKEY")" \
    "$trigger" \
    "$TIMEOUT" \
    "$(field app "${APP_PATH:-planned}")"
}

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ validate macOS host and Accessibility automation"
  if [ -n "$DMG" ]; then
    echo "+ mount $DMG, copy Cerul.app to a temporary install directory"
  elif [ -n "$APP_PATH" ]; then
    echo "+ copy $APP_PATH to a temporary install directory"
  else
    echo "+ locate latest target/electron/*.dmg and copy Cerul.app"
  fi
  echo "+ launch copied Cerul.app with isolated HOME, stripped PATH, and CERUL_GLOBAL_HOTKEY=$HOTKEY"
  echo "+ wait for $API_HEALTH_URL"
  if [ "$MANUAL_TRIGGER" -eq 1 ]; then
    echo "+ wait up to ${TIMEOUT}s for a physical $HOTKEY press"
  else
    echo "+ send $HOTKEY through System Events"
  fi
  echo "+ verify a visible Cerul window appears"
  echo "+ send Escape and verify the visible Cerul window hides"
  emit_result planned
  exit 0
fi

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Installed hotkey smoke requires macOS." >&2
  exit 2
fi

if [ ! -x "$CURL_BIN" ]; then
  if command -v curl >/dev/null 2>&1; then
    CURL_BIN="$(command -v curl)"
  else
    echo "curl is required to validate the installed runtime health endpoint." >&2
    exit 2
  fi
fi

if [ -z "$DMG" ] && [ -z "$APP_PATH" ] && [ -d "$ROOT/target" ]; then
  DMG="$(find "$ROOT/target/electron" -maxdepth 2 -name "*.dmg" -type f -print 2>/dev/null | sort | tail -1)"
fi

if [ -z "$DMG" ] && [ -z "$APP_PATH" ]; then
  echo "No app target found. Run scripts/build-installers.sh first, pass --dmg, or pass --app." >&2
  exit 1
fi

MOUNT_DIR=""
HOME_DIR="$(mktemp -d)"
INSTALL_DIR="$(mktemp -d)"
PID=""
ATTACH_LOG=""
AUTOMATION_LOG=""

cleanup() {
  if [ -n "$PID" ]; then
    kill "$PID" >/dev/null 2>&1 || true
    wait "$PID" >/dev/null 2>&1 || true
  fi
  if [ -n "$MOUNT_DIR" ]; then
    hdiutil detach "$MOUNT_DIR" -quiet >/dev/null 2>&1 || \
      hdiutil detach "$MOUNT_DIR" -force -quiet >/dev/null 2>&1 || true
    rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
  rm -rf "$HOME_DIR" "$INSTALL_DIR" "$ATTACH_LOG" "$AUTOMATION_LOG" || true
}
trap cleanup EXIT

if [ -n "$DMG" ]; then
  if [ ! -f "$DMG" ]; then
    echo "DMG not found: $DMG" >&2
    exit 1
  fi

  MOUNT_DIR="$(mktemp -d)"
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
fi

if [ ! -d "$APP_PATH" ]; then
  echo "Cerul.app not found: $APP_PATH" >&2
  exit 1
fi

INSTALLED_APP_PATH="$INSTALL_DIR/Cerul.app"
if ! ditto "$APP_PATH" "$INSTALLED_APP_PATH"; then
  echo "Failed to copy Cerul.app to temporary install directory." >&2
  exit 1
fi

APP_PATH="$INSTALLED_APP_PATH"
BIN_PATH="$(find "$APP_PATH/Contents/MacOS" -maxdepth 1 -type f -print -quit)"
if [ -z "$BIN_PATH" ] || [ ! -x "$BIN_PATH" ]; then
  echo "Cerul.app does not contain an executable in Contents/MacOS." >&2
  exit 1
fi

if "$CURL_BIN" -fsS --max-time 1 "$API_HEALTH_URL" >/dev/null 2>&1; then
  echo "Cerul Core already responds at $API_HEALTH_URL before launch; stop the existing runtime and rerun." >&2
  exit 1
fi

visible_cerul_window_count() {
  /usr/bin/osascript <<'APPLESCRIPT'
tell application "System Events"
  set total to 0
  set cerulProcesses to {}
  try
    set cerulProcesses to every application process whose bundle identifier is "ai.cerul.desktop"
  end try
  if (count of cerulProcesses) is 0 then
    try
      set cerulProcesses to every application process whose name contains "Cerul"
    end try
  end if
  repeat with cerulProcess in cerulProcesses
    repeat with cerulWindow in windows of cerulProcess
      try
        if visible of cerulWindow is true then set total to total + 1
      end try
    end repeat
  end repeat
  return total
end tell
APPLESCRIPT
}

send_hotkey() {
  case "$HOTKEY" in
    Alt+Space)
      /usr/bin/osascript -e 'tell application "System Events" to key code 49 using {option down}'
      ;;
    Ctrl+Space)
      /usr/bin/osascript -e 'tell application "System Events" to key code 49 using {control down}'
      ;;
    Ctrl+Shift+Space)
      /usr/bin/osascript -e 'tell application "System Events" to key code 49 using {control down, shift down}'
      ;;
    Cmd+Shift+Space)
      /usr/bin/osascript -e 'tell application "System Events" to key code 49 using {command down, shift down}'
      ;;
  esac
}

send_escape() {
  /usr/bin/osascript -e 'tell application "System Events" to key code 53'
}

AUTOMATION_LOG="$(mktemp)"
if ! visible_cerul_window_count >"$AUTOMATION_LOG" 2>&1; then
  cat "$AUTOMATION_LOG" >&2
  echo "System Events window inspection failed. Grant Accessibility permission to this runner and rerun." >&2
  exit 1
fi

env -i HOME="$HOME_DIR" PATH="/usr/bin:/bin" CERUL_GLOBAL_HOTKEY="$HOTKEY" "$BIN_PATH" &
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

sleep 1

before_count="$(visible_cerul_window_count)"
if [ "$MANUAL_TRIGGER" -eq 1 ]; then
  echo "Manual hotkey smoke armed. Press $HOTKEY within ${TIMEOUT}s."
else
  if ! send_hotkey >"$AUTOMATION_LOG" 2>&1; then
    cat "$AUTOMATION_LOG" >&2
    echo "System Events could not send $HOTKEY. Grant Accessibility permission to this runner and rerun." >&2
    exit 1
  fi
fi

deadline=$((SECONDS + TIMEOUT))
shown_count=0
while [ "$SECONDS" -lt "$deadline" ]; do
  shown_count="$(visible_cerul_window_count)"
  if [ "$shown_count" -gt "$before_count" ]; then
    break
  fi
  sleep 1
done

if [ "$shown_count" -le "$before_count" ]; then
  echo "Cerul overlay did not become visible within ${TIMEOUT}s after $HOTKEY. Before=$before_count after=$shown_count." >&2
  if [ "$MANUAL_TRIGGER" -eq 1 ]; then
    echo "The physical $HOTKEY press was not observed by the installed Cerul app. Confirm the app registered the same hotkey and macOS did not reserve it for another app." >&2
  else
    echo "If the app log above does not include the overlay shortcut event, macOS did not deliver the synthetic hotkey to Cerul; rerun with --manual from an interactive runner and press the configured physical hotkey." >&2
  fi
  exit 1
fi

if ! send_escape >"$AUTOMATION_LOG" 2>&1; then
  cat "$AUTOMATION_LOG" >&2
  echo "System Events could not send Escape. Grant Accessibility permission to this runner and rerun." >&2
  exit 1
fi

deadline=$((SECONDS + TIMEOUT))
hidden_count="$shown_count"
while [ "$SECONDS" -lt "$deadline" ]; do
  hidden_count="$(visible_cerul_window_count)"
  if [ "$hidden_count" -le "$before_count" ]; then
    break
  fi
  sleep 1
done

if [ "$hidden_count" -gt "$before_count" ]; then
  echo "Cerul overlay did not hide within ${TIMEOUT}s after Escape. Before=$before_count after=$hidden_count." >&2
  exit 1
fi

emit_result passed
echo "Installed hotkey smoke passed for $APP_PATH with hotkey=$HOTKEY before_windows=$before_count shown_windows=$shown_count hidden_windows=$hidden_count"
