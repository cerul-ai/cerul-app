#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG=""
APP=""
EXPECTED_TEAM_ID="${APPLE_TEAM_ID:-}"
SKIP_NOTARIZATION=0
ALLOW_AD_HOC=0
DRY_RUN=0
MOUNT_DIR=""
MAX_CODESIGN_XATTR_FILES="${CERUL_MAX_CODESIGN_XATTR_FILES:-1000}"
GATEKEEPER_NOFILE_LIMIT="${CERUL_GATEKEEPER_NOFILE_LIMIT:-256}"

usage() {
  cat <<'EOF'
Usage: scripts/smoke-macos-signing.sh [--dmg <path>] [--app <path>] [--expected-team-id <id>] [--allow-ad-hoc] [--skip-notarization] [--dry-run]

Verifies the macOS public-release signing gate. With --dmg, the script mounts
the image, verifies the embedded Cerul.app Developer ID signature and release
entitlements, checks Gatekeeper assessment, and validates the stapled
notarization ticket. With --app, only the app signature/Gatekeeper checks run
unless --dmg is also set.

Use --allow-ad-hoc only for unsigned internal/alpha artifacts. It still verifies
that the app bundle has a complete code signature, but accepts an ad-hoc
signature and skips Gatekeeper/notarization acceptance checks because macOS is
expected to show "unidentified developer" for that distribution mode.
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
    --allow-ad-hoc)
      ALLOW_AD_HOC=1
      shift
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

run_low_nofile() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+ ulimit -n %q;' "$GATEKEEPER_NOFILE_LIMIT"
    printf ' %q' "$@"
    printf '\n'
  else
    (ulimit -n "$GATEKEEPER_NOFILE_LIMIT" 2>/dev/null || true; "$@")
  fi
}

require_codesign_xattr_budget() {
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ verify signed resource xattr count <= $MAX_CODESIGN_XATTR_FILES"
    return
  fi

  if ! [[ "$MAX_CODESIGN_XATTR_FILES" =~ ^[0-9]+$ ]]; then
    echo "CERUL_MAX_CODESIGN_XATTR_FILES must be a non-negative integer." >&2
    exit 2
  fi

  local signed_xattr_count=0
  local first_over_budget_file=""
  while IFS= read -r file; do
    if xattr -p com.apple.cs.CodeSignature "$file" >/dev/null 2>&1; then
      signed_xattr_count=$((signed_xattr_count + 1))
      if [ "$signed_xattr_count" -gt "$MAX_CODESIGN_XATTR_FILES" ]; then
        first_over_budget_file="$file"
        break
      fi
    fi
  done < <(find "$APP" -type f -print 2>/dev/null)

  if [ "$signed_xattr_count" -gt "$MAX_CODESIGN_XATTR_FILES" ]; then
    echo "Cerul.app has $signed_xattr_count files with com.apple.cs.CodeSignature xattrs, above budget $MAX_CODESIGN_XATTR_FILES." >&2
    echo "This can make macOS Gatekeeper fail with 'Too many open files' and show the DMG as damaged." >&2
    echo "First file over budget: $first_over_budget_file" >&2
    exit 1
  fi
  echo "codesign_xattr_budget signed_files=$signed_xattr_count max=$MAX_CODESIGN_XATTR_FILES"
}

require_entitlement() {
  local subject="$1"
  local key="$2"

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ verify entitlement $key on $subject"
    return
  fi

  local entitlements
  entitlements="$(codesign -d --entitlements :- "$subject" 2>/dev/null || true)"
  if ! printf '%s\n' "$entitlements" | grep -q "<key>$key</key>"; then
    echo "Missing required macOS entitlement $key on $subject." >&2
    printf '%s\n' "$entitlements" >&2
    exit 1
  fi
}

require_release_entitlements() {
  local app_exec="$APP/Contents/MacOS/Cerul"
  local runtime_python="$APP/Contents/Resources/mlx-runtime/bin/python3.12"

  if [ "$DRY_RUN" -eq 0 ]; then
    if [ ! -x "$app_exec" ]; then
      echo "Cerul app executable not found: $app_exec" >&2
      exit 1
    fi
    if [ ! -x "$runtime_python" ]; then
      echo "Bundled MLX Python interpreter not found: $runtime_python" >&2
      exit 1
    fi
  fi

  for subject in "$app_exec" "$runtime_python"; do
    require_entitlement "$subject" "com.apple.security.cs.allow-jit"
    require_entitlement "$subject" "com.apple.security.cs.allow-unsigned-executable-memory"
    require_entitlement "$subject" "com.apple.security.cs.disable-library-validation"
  done
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

require_codesign_xattr_budget
run codesign --verify --deep --strict --verbose=2 "$APP"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ codesign -dv --verbose=4 $APP"
  if [ "$ALLOW_AD_HOC" -eq 1 ]; then
    identity="Signature=adhoc
TeamIdentifier=not set"
  else
    identity="Authority=Developer ID Application: dry run
TeamIdentifier=${EXPECTED_TEAM_ID:-DRYRUNTEAM}"
  fi
else
  identity="$(
    codesign -dv --verbose=4 "$APP" 2>&1 || true
  )"
fi

if [ "$DRY_RUN" -eq 0 ]; then
  if printf '%s\n' "$identity" | grep -q '^Authority=Developer ID Application:'; then
    signing_mode="developer_id"
  elif [ "$ALLOW_AD_HOC" -eq 1 ] &&
    printf '%s\n' "$identity" | grep -q '^Signature=adhoc$'; then
    signing_mode="ad_hoc"
  else
    echo "Cerul.app is not signed with a Developer ID Application certificate." >&2
    if [ "$ALLOW_AD_HOC" -eq 1 ]; then
      echo "Cerul.app is also not ad-hoc signed; unsigned alpha artifacts must still have a valid ad-hoc signature." >&2
    fi
    printf '%s\n' "$identity" >&2
    exit 1
  fi
else
  signing_mode="$([ "$ALLOW_AD_HOC" -eq 1 ] && echo ad_hoc || echo developer_id)"
fi

if [ "$DRY_RUN" -eq 0 ] && [ "$signing_mode" = "developer_id" ]; then
  if [ -n "$EXPECTED_TEAM_ID" ] &&
    ! printf '%s\n' "$identity" | grep -q "^TeamIdentifier=$EXPECTED_TEAM_ID$"; then
    echo "Cerul.app TeamIdentifier does not match expected Apple team id $EXPECTED_TEAM_ID." >&2
    printf '%s\n' "$identity" >&2
    exit 1
  fi
fi

if [ "$signing_mode" = "developer_id" ]; then
  require_release_entitlements
  run_low_nofile spctl --assess --type execute --verbose "$APP"
else
  echo "Skipping Gatekeeper assessment for ad-hoc alpha artifact; macOS is expected to report an unidentified developer."
fi

if [ -n "$DMG" ] && [ "$SKIP_NOTARIZATION" -eq 0 ] && [ "$signing_mode" = "developer_id" ]; then
  run_low_nofile spctl --assess --type open --context context:primary-signature --verbose "$DMG"
  run xcrun stapler validate "$DMG"
fi

notarization_checked=0
if [ "$signing_mode" = "developer_id" ]; then
  notarization_checked=$((1 - SKIP_NOTARIZATION))
fi

echo "macos_signing_smoke app=$APP dmg=${DMG:-none} signing_mode=$signing_mode notarization_checked=$notarization_checked"
