#!/usr/bin/env bash
set -euo pipefail

API_HEALTH_URL="${CERUL_API_HEALTH_URL:-http://127.0.0.1:7777/health}"
CURL_BIN="${CURL_BIN:-/usr/bin/curl}"
TIMEOUT_SECONDS="${CERUL_BOOT_SMOKE_TIMEOUT:-30}"
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-boot-persistence-macos.sh [--health-url <url>] [--timeout <seconds>] [--dry-run]

Run after a macOS reboot/login to record release evidence for the
boot-persistence smoke. Electron owns Start at login
through app.setLoginItemSettings(), so this smoke is read-only and verifies that
the installed app came up and its local API is healthy after login.

Environment overrides:
  CERUL_API_HEALTH_URL         default: http://127.0.0.1:7777/health
  CERUL_BOOT_SMOKE_TIMEOUT     default: 30
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --label|--plist)
      echo "$1 is no longer used; Electron Start at login is not LaunchAgent-plist based." >&2
      exit 2
      ;;
    --health-url)
      API_HEALTH_URL="${2:?missing API health URL}"
      shift 2
      ;;
    --timeout)
      TIMEOUT_SECONDS="${2:?missing timeout seconds}"
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

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Boot persistence smoke currently applies only to macOS installed apps." >&2
  exit 2
fi

field() {
  local key="$1"
  local value="$2"
  printf '%s=%q' "$key" "$value"
}

emit_result() {
  local status="$1"
  printf 'boot_persistence_smoke status=%s owner=electron_login_item %s timeout=%ss\n' \
    "$status" \
    "$(field health_url "$API_HEALTH_URL")" \
    "$TIMEOUT_SECONDS"
}

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ ensure Start at login was enabled in the installed Electron app before reboot"
  echo "+ poll $API_HEALTH_URL for ${TIMEOUT_SECONDS}s after login"
  emit_result planned
  exit 0
fi

if [ ! -x "$CURL_BIN" ]; then
  if command -v curl >/dev/null 2>&1; then
    CURL_BIN="$(command -v curl)"
  else
    echo "curl is required to validate the installed runtime health endpoint." >&2
    exit 2
  fi
fi

deadline=$((SECONDS + TIMEOUT_SECONDS))
health_body=""
while [ "$SECONDS" -lt "$deadline" ]; do
  if health_body="$("$CURL_BIN" -fsS --max-time 2 "$API_HEALTH_URL" 2>/dev/null)" &&
    printf '%s' "$health_body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"'; then
    echo "Installed runtime health: $health_body"
    emit_result passed
    echo "Boot persistence smoke passed."
    exit 0
  fi

  sleep 1
done

echo "Cerul did not report healthy at $API_HEALTH_URL within ${TIMEOUT_SECONDS}s after login." >&2
exit 1
