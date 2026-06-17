#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE_ROOT="$ROOT/target/electron"
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/finalize-macos-release-artifacts.sh [--bundle-root <path>] [--dry-run]

Signs, notarizes, and staples final macOS DMG artifacts, then regenerates
macOS auto-update metadata and blockmaps from the final bytes.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle-root)
      BUNDLE_ROOT="${2:?missing bundle root}"
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

BUNDLE_ROOT="$(cd "$BUNDLE_ROOT" && pwd)"

if [ "$DRY_RUN" -eq 0 ]; then
  if [ "$(uname -s)" != "Darwin" ]; then
    echo "Final macOS release artifact signing requires macOS." >&2
    exit 2
  fi
  command -v codesign >/dev/null 2>&1 || {
    echo "codesign is required to finalize macOS release artifacts." >&2
    exit 2
  }
  command -v xcrun >/dev/null 2>&1 || {
    echo "xcrun is required to notarize macOS release artifacts." >&2
    exit 2
  }
fi

identity="${APPLE_SIGNING_IDENTITY:-${CSC_NAME:-Developer ID Application: dry run}}"
apple_id="${APPLE_ID:-dry-run@example.invalid}"
password="${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_PASSWORD:-dry-run-password}}"
team_id="${APPLE_TEAM_ID:-DRYRUNTEAM}"

if [ "$DRY_RUN" -eq 0 ]; then
  if [ -z "${APPLE_SIGNING_IDENTITY:-${CSC_NAME:-}}" ]; then
    echo "APPLE_SIGNING_IDENTITY or CSC_NAME is required to sign final macOS DMGs." >&2
    exit 2
  fi
  : "${APPLE_ID:?APPLE_ID is required to notarize final macOS DMGs}"
  if [ -z "${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_PASSWORD:-}}" ]; then
    echo "APPLE_APP_SPECIFIC_PASSWORD is required to notarize final macOS DMGs." >&2
    exit 2
  fi
  : "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required to notarize final macOS DMGs}"
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

timer_now() {
  date +%s
}

time_step() {
  local name="$1"
  shift
  local start
  local end
  local status
  start="$(timer_now)"
  echo "release_timing_start step=$name epoch=$start"
  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    echo "::group::$name"
  fi
  set +e
  "$@"
  status=$?
  set -e
  end="$(timer_now)"
  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    echo "::endgroup::"
  fi
  echo "release_timing step=$name seconds=$((end - start)) status=$status"
  return "$status"
}

submit_notary() {
  local dmg="$1"
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+ xcrun notarytool submit %q --apple-id %q --password %q --team-id %q --wait\n' \
      "$dmg" "$apple_id" "***" "$team_id"
  else
    xcrun notarytool submit "$dmg" \
      --apple-id "$apple_id" \
      --password "$password" \
      --team-id "$team_id" \
      --wait
  fi
}

staple_dmg() {
  local dmg="$1"

  if [ "$DRY_RUN" -eq 1 ]; then
    run xcrun stapler staple "$dmg"
    run xcrun stapler validate "$dmg"
    return
  fi

  local attempt
  for attempt in 1 2 3 4 5; do
    if xcrun stapler staple "$dmg"; then
      xcrun stapler validate "$dmg"
      return
    fi
    sleep $((attempt * 10))
  done

  echo "Failed to staple notarization ticket to $dmg after retries." >&2
  exit 1
}

dmgs=()
while IFS= read -r dmg; do
  dmgs+=("$dmg")
done < <(find "$BUNDLE_ROOT" -maxdepth 1 -type f -name "*.dmg" -print | sort)
if [ "${#dmgs[@]}" -eq 0 ]; then
  echo "No DMG artifacts found under $BUNDLE_ROOT." >&2
  exit 1
fi

for dmg in "${dmgs[@]}"; do
  time_step dmg_sign run codesign --force --sign "$identity" --timestamp "$dmg"
  time_step dmg_sign_verify run codesign --verify --verbose=2 "$dmg"
  if [ "${CERUL_NOTARIZE:-0}" = "1" ]; then
    time_step dmg_notarization submit_notary "$dmg"
    time_step dmg_staple staple_dmg "$dmg"
  else
    echo "CERUL_NOTARIZE is not 1; leaving $dmg signed but not notarized."
  fi
done

metadata_args=(--bundle-root "$BUNDLE_ROOT")
if [ "$DRY_RUN" -eq 1 ]; then
  metadata_args+=(--dry-run)
fi
time_step macos_update_metadata run node "$ROOT/scripts/regenerate-macos-update-metadata.cjs" "${metadata_args[@]}"

echo "finalize_macos_release_artifacts status=passed bundle_root=$BUNDLE_ROOT dmg_count=${#dmgs[@]}"
