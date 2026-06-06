#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG=""
APP=""
EXPECTED_TEAM_ID="${APPLE_TEAM_ID:-}"
SKIP_NOTARIZATION=0
DRY_RUN=0
MOUNT_DIR=""

usage() {
  cat <<'EOF'
Usage: scripts/smoke-macos-signing.sh [--dmg <path>] [--app <path>] [--expected-team-id <id>] [--skip-notarization] [--dry-run]

Verifies the macOS public-release signing gate. With --dmg, the script mounts
the image, verifies the embedded Cerul.app Developer ID signature, checks
Gatekeeper assessment, and validates the stapled notarization ticket. With
--app, only the app signature/Gatekeeper checks run unless --dmg is also set.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dmg)
      DMG="${2:?missing DMG path}"
      shift 2
      ;;
    --app)
      APP="${2:?missing app path}"
      shift 2
      ;;
    --expected-team-id)
      EXPECTED_TEAM_ID="${2:?missing Apple team id}"
      shift 2
      ;;
    --skip-notarization)
      SKIP_NOTARIZATION=1
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

if [ "$(uname -s)" != "Darwin" ]; then
  echo "macOS signing smoke requires macOS codesign/spctl tooling." >&2
  exit 2
fi

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

cleanup() {
  if [ -n "$MOUNT_DIR" ]; then
    hdiutil detach "$MOUNT_DIR" -quiet >/dev/null 2>&1 || \
      hdiutil detach "$MOUNT_DIR" -force -quiet >/dev/null 2>&1 || true
    rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [ -z "$DMG" ] && [ -z "$APP" ] && [ -d "$ROOT/target" ]; then
  DMG="$(find "$ROOT/target" -path "*/bundle/dmg/*.dmg" -type f -print | sort | tail -1)"
fi

if [ -z "$DMG" ] && [ -z "$APP" ]; then
  echo "No DMG or app path provided. Run scripts/build-installers.sh with signing first, or pass --dmg/--app." >&2
  exit 1
fi

if [ -n "$DMG" ]; then
  if [ ! -f "$DMG" ] && [ "$DRY_RUN" -eq 0 ]; then
    echo "DMG not found: $DMG" >&2
    exit 1
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ mount $DMG and discover Cerul.app"
    APP="${APP:-/Volumes/Cerul/Cerul.app}"
  elif [ -z "$APP" ]; then
    MOUNT_DIR="$(mktemp -d)"
    hdiutil attach "$DMG" -mountpoint "$MOUNT_DIR" -nobrowse -readonly >/dev/null
    APP="$(find "$MOUNT_DIR" -maxdepth 2 -name "Cerul.app" -type d -print -quit)"
    if [ -z "$APP" ]; then
      echo "Mounted DMG did not contain Cerul.app." >&2
      exit 1
    fi
  fi
fi

if [ ! -d "$APP" ] && [ "$DRY_RUN" -eq 0 ]; then
  echo "App bundle not found: $APP" >&2
  exit 1
fi

run codesign --verify --deep --strict --verbose=2 "$APP"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ codesign -dv --verbose=4 $APP"
  identity="Authority=Developer ID Application: dry run
TeamIdentifier=${EXPECTED_TEAM_ID:-DRYRUNTEAM}"
else
  identity="$(
    codesign -dv --verbose=4 "$APP" 2>&1 || true
  )"
fi

if [ "$DRY_RUN" -eq 0 ]; then
  if ! printf '%s\n' "$identity" | grep -q '^Authority=Developer ID Application:'; then
    echo "Cerul.app is not signed with a Developer ID Application certificate." >&2
    printf '%s\n' "$identity" >&2
    exit 1
  fi

  if [ -n "$EXPECTED_TEAM_ID" ] &&
    ! printf '%s\n' "$identity" | grep -q "^TeamIdentifier=$EXPECTED_TEAM_ID$"; then
    echo "Cerul.app TeamIdentifier does not match expected Apple team id $EXPECTED_TEAM_ID." >&2
    printf '%s\n' "$identity" >&2
    exit 1
  fi
fi

run spctl --assess --type execute --verbose "$APP"

if [ -n "$DMG" ] && [ "$SKIP_NOTARIZATION" -eq 0 ]; then
  run spctl --assess --type open --context context:primary-signature --verbose "$DMG"
  run xcrun stapler validate "$DMG"
fi

echo "macos_signing_smoke app=$APP dmg=${DMG:-none} notarization_checked=$((1 - SKIP_NOTARIZATION))"
