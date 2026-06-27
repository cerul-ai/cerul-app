#!/usr/bin/env bash
set -euo pipefail

# Reset Cerul's local state so the next launch shows the fresh first-run
# experience: an empty library plus the onboarding intro (the "boot animation").
#
# Nothing is deleted — each target is MOVED ASIDE into a timestamped backup next
# to it, matching the repo's existing "move aside" convention. A reset is fully
# reversible: move the *.reset-backup-* directory back to restore.
#
# What gets reset (macOS paths):
#   - Data dir:           ~/Library/Application Support/Cerul
#                         (cerul.db, indexes, models, cache, provider-keys.json)
#   - Packaged userData:  ~/Library/Application Support/@cerul/electron-shell
#                         (lastRoute, onboarding flag, window bounds, cloud auth)
#   - Dev userData store:  ~/Library/Application Support/Electron/stores
#                         (same keys for ./run.sh dev builds; only Cerul's
#                          stores/ is moved — the shared Electron dir, which
#                          other unbranded Electron dev apps also use, is left
#                          intact)
#
# Usage:
#   scripts/reset-local-data.sh                 # dry run — print what would change
#   scripts/reset-local-data.sh --apply         # perform the reset (refuses if Cerul is running)
#   scripts/reset-local-data.sh --apply --quit  # gracefully quit a running Cerul first, then reset

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This helper currently only knows macOS paths." >&2
  exit 1
fi

APPLY=0
QUIT=0
for arg in "$@"; do
  case "$arg" in
    --apply) APPLY=1 ;;
    --quit) QUIT=1 ;;
    -h|--help)
      sed -n '3,33p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      echo "Usage: scripts/reset-local-data.sh [--apply] [--quit]" >&2
      exit 2
      ;;
  esac
done

SUPPORT="$HOME/Library/Application Support"
STAMP="$(date +%Y%m%d-%H%M%S)"

# Cerul-specific targets. The dev userData ("Electron") is shared with other
# unbranded Electron dev apps, so only Cerul's stores/ subtree is listed.
TARGETS=(
  "$SUPPORT/Cerul"
  "$SUPPORT/@cerul/electron-shell"
  "$SUPPORT/Electron/stores"
)

is_running() {
  pgrep -f "/Applications/Cerul.app/Contents/MacOS/Cerul" >/dev/null 2>&1 ||
    pgrep -f "Resources/bin/cerul-core" >/dev/null 2>&1 ||
    pgrep -f "Resources/bin/cerul-api" >/dev/null 2>&1 ||
    pgrep -f "target/(debug|release)/cerul-api" >/dev/null 2>&1
}

if is_running; then
  if [ "$QUIT" = "1" ] && [ "$APPLY" = "1" ]; then
    echo "Quitting Cerul (graceful)…"
    osascript -e 'quit app "Cerul"' >/dev/null 2>&1 || true
    pkill -f "/Applications/Cerul.app/Contents/MacOS/Cerul" >/dev/null 2>&1 || true
    # Up to ~12s of grace: the backend owns the vector index, whose WAL may be mid-flush.
    for _ in $(seq 1 60); do
      is_running || break
      sleep 0.2
    done
  fi
  if is_running; then
    echo "Cerul is still running." >&2
    echo "Quit it first, or re-run with: scripts/reset-local-data.sh --apply --quit" >&2
    exit 1
  fi
fi

if [ "$APPLY" = "1" ]; then
  echo "Resetting Cerul local state (backups suffixed .reset-backup-${STAMP}):"
else
  echo "DRY RUN — no changes. Pass --apply to perform the reset."
fi

moved_any=0
for target in "${TARGETS[@]}"; do
  if [ -e "$target" ]; then
    dest="${target}.reset-backup-${STAMP}"
    if [ "$APPLY" = "1" ]; then
      mv "$target" "$dest"
      echo "  moved:   $target"
      echo "        -> $dest"
      moved_any=1
    else
      echo "  would move: $target"
    fi
  else
    echo "  absent (skip): $target"
  fi
done

if [ "$APPLY" = "1" ]; then
  if [ "$moved_any" = "1" ]; then
    echo "Done. Next launch starts fresh: empty library + onboarding intro."
  else
    echo "Nothing to reset — all targets were already absent."
  fi
else
  echo "Re-run with --apply (add --quit if Cerul is open) to perform it."
fi
