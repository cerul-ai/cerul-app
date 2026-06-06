#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BINARY=""
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-daemon-autostart-windows.sh [--binary <path>] [--dry-run]

Runs the packaged Electron binary's login-item CLI in an isolated smoke mode:

  - --install-daemon reports installed=true
  - --daemon-status reports installed=true
  - --uninstall-daemon reports installed=false
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

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ run packaged Windows login-item CLI smoke"
  echo "daemon_autostart_smoke platform=windows status=planned owner=electron_login_item"
  exit 0
fi

if [ -z "$BINARY" ]; then
  BINARY="$(find "$ROOT/target/electron" -type f -path "*/win-unpacked/Cerul.exe" -print -quit 2>/dev/null || true)"
fi

if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "Cerul.exe was not found. Pass --binary or build win-unpacked first." >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

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
  echo "Windows login-item install smoke failed:" >&2
  echo "$INSTALL_OUTPUT" >&2
  exit 1
fi

STATUS_OUTPUT="$(run_login_command --daemon-status)"
if [ "$(json_field "$STATUS_OUTPUT" "data.installed")" != "true" ]; then
  echo "Windows login-item status smoke failed:" >&2
  echo "$STATUS_OUTPUT" >&2
  exit 1
fi

UNINSTALL_OUTPUT="$(run_login_command --uninstall-daemon)"
if [ "$(json_field "$UNINSTALL_OUTPUT" "data.installed")" != "false" ]; then
  echo "Windows login-item uninstall smoke failed:" >&2
  echo "$UNINSTALL_OUTPUT" >&2
  exit 1
fi

echo "daemon_autostart_smoke platform=windows status=passed binary=$BINARY"
