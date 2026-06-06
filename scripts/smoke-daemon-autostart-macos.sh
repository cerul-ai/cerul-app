#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DMG=""
APP_PATH=""
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-daemon-autostart-macos.sh [--dmg <path> | --app <Cerul.app>] [--dry-run]

Runs the packaged Electron app's login-item CLI in an isolated smoke mode:

  - --install-daemon reports installed=true
  - --daemon-status reports installed=true
  - --uninstall-daemon reports installed=false

Pass --dmg for release artifacts or --app for an unpacked Cerul.app.
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

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ run packaged macOS login-item CLI smoke"
  echo "daemon_autostart_smoke platform=macos status=planned owner=electron_login_item"
  exit 0
fi

TMP_DIR="$(mktemp -d)"
MOUNT_POINT=""
cleanup() {
  if [ -n "$MOUNT_POINT" ]; then
    hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

if [ -n "$DMG" ]; then
  if [ ! -f "$DMG" ]; then
    echo "DMG does not exist: $DMG" >&2
    exit 2
  fi
  MOUNT_POINT="$TMP_DIR/dmg"
  mkdir -p "$MOUNT_POINT"
  hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MOUNT_POINT" >/dev/null
  APP_PATH="$(find "$MOUNT_POINT" -maxdepth 2 -type d -name "Cerul.app" -print -quit)"
elif [ -z "$APP_PATH" ]; then
  APP_PATH="$(find "$ROOT/target/electron" -type d -name "Cerul.app" -print -quit 2>/dev/null || true)"
fi

if [ -z "$APP_PATH" ] || [ ! -d "$APP_PATH" ]; then
  echo "Cerul.app was not found. Pass --app or --dmg." >&2
  exit 2
fi

BINARY="$(find "$APP_PATH/Contents/MacOS" -maxdepth 1 -type f -perm -111 -print -quit 2>/dev/null || true)"
if [ -z "$BINARY" ]; then
  echo "Cerul.app does not contain an executable in Contents/MacOS: $APP_PATH" >&2
  exit 1
fi

SMOKE_FILE="$TMP_DIR/login-item.json"

run_login_command() {
  CERUL_LOGIN_ITEM_SMOKE_FILE="$SMOKE_FILE" "$BINARY" "$@"
}

json_field() {
  node -e '
    const lines = process.argv[1].trim().split(/\n/).filter(Boolean);
    const jsonLine = [...lines].reverse().find((line) => line.trim().startsWith("{"));
    if (!jsonLine) process.exit(3);
    const data = JSON.parse(jsonLine);
    const value = Function("data", `return (${process.argv[2]});`)(data);
    if (value === undefined || value === null) process.exit(3);
    process.stdout.write(String(value));
  ' "$1" "$2"
}

INSTALL_OUTPUT="$(run_login_command --install-daemon)"
if [ "$(json_field "$INSTALL_OUTPUT" "data.installed")" != "true" ]; then
  echo "macOS login-item install smoke failed:" >&2
  echo "$INSTALL_OUTPUT" >&2
  exit 1
fi

STATUS_OUTPUT="$(run_login_command --daemon-status)"
if [ "$(json_field "$STATUS_OUTPUT" "data.installed")" != "true" ]; then
  echo "macOS login-item status smoke failed:" >&2
  echo "$STATUS_OUTPUT" >&2
  exit 1
fi

UNINSTALL_OUTPUT="$(run_login_command --uninstall-daemon)"
if [ "$(json_field "$UNINSTALL_OUTPUT" "data.installed")" != "false" ]; then
  echo "macOS login-item uninstall smoke failed:" >&2
  echo "$UNINSTALL_OUTPUT" >&2
  exit 1
fi

echo "daemon_autostart_smoke platform=macos status=passed app=$APP_PATH binary=$BINARY"
