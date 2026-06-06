#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=""
DEBUG=0
NO_BUNDLE=0
SKIP_FETCH=0
DRY_RUN=0
REQUIRE_SIGNING=0

usage() {
  cat <<'EOF'
Usage: scripts/build-installers.sh [--target <triple>] [--debug] [--no-bundle] [--skip-fetch] [--require-signing] [--dry-run]

Builds Cerul installers with Electron. The build contract is:
  1. build the React renderer
  2. build release cerul-api
  3. stage cerul-api into apps/electron-shell/bin/
  4. run electron-builder, which copies bin/, desktop-dist/, third-party/, and mlx-sidecar/

Signing is handled by electron-builder. For public macOS release candidates,
provide Developer ID and notarization credentials through CI secrets or local
environment variables, then pass --require-signing:
  APPLE_SIGNING_IDENTITY="Developer ID Application: ..." CERUL_NOTARIZE=1 APPLE_ID=... APPLE_APP_SPECIFIC_PASSWORD=... APPLE_TEAM_ID=... scripts/build-installers.sh --require-signing

Windows signing is intentionally external to this script until a certificate is
available; the release workflow can upload unsigned NSIS artifacts.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      TARGET="${2:?missing target}"
      shift 2
      ;;
    --debug)
      DEBUG=1
      shift
      ;;
    --no-bundle)
      NO_BUNDLE=1
      shift
      ;;
    --skip-fetch)
      SKIP_FETCH=1
      shift
      ;;
    --require-signing)
      REQUIRE_SIGNING=1
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

cd "$ROOT"

target_is_macos() {
  if [ -n "$TARGET" ]; then
    case "$TARGET" in
      *apple-darwin) return 0 ;;
      *) return 1 ;;
    esac
  fi

  [ "$(uname -s)" = "Darwin" ]
}

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

require_command() {
  local name="$1"

  if ! command -v "$name" >/dev/null 2>&1; then
    echo "$name is required when --require-signing is used." >&2
    exit 2
  fi
}

check_signing_prereqs() {
  if [ "$REQUIRE_SIGNING" -eq 0 ]; then
    return
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ check macOS Developer ID signing and notarization prerequisites"
    return
  fi

  if ! target_is_macos; then
    echo "--require-signing only applies to macOS targets." >&2
    exit 2
  fi

  if [ "$(uname -s)" != "Darwin" ]; then
    echo "--require-signing requires a macOS host with Apple signing tools." >&2
    exit 2
  fi

  : "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY is required when --require-signing is used}"
  if [ "${CERUL_NOTARIZE:-0}" != "1" ]; then
    echo "CERUL_NOTARIZE=1 is required when --require-signing is used." >&2
    exit 2
  fi
  : "${APPLE_ID:?APPLE_ID is required when --require-signing is used}"
  if [ -z "${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_PASSWORD:-}}" ]; then
    echo "APPLE_APP_SPECIFIC_PASSWORD is required when --require-signing is used." >&2
    exit 2
  fi
  : "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required when --require-signing is used}"

  require_command codesign
  require_command security
  require_command xcrun

  if ! security find-identity -v -p codesigning | grep -F "$APPLE_SIGNING_IDENTITY" >/dev/null; then
    echo "APPLE_SIGNING_IDENTITY was not found in the active macOS keychains: $APPLE_SIGNING_IDENTITY" >&2
    exit 1
  fi

  xcrun -f notarytool >/dev/null
  xcrun -f stapler >/dev/null
  export CSC_NAME="${CSC_NAME:-$APPLE_SIGNING_IDENTITY}"
  export APPLE_APP_SPECIFIC_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD:-$APPLE_PASSWORD}"
}

electron_builder_args() {
  if [ -z "$TARGET" ]; then
    return
  fi

  case "$TARGET" in
    aarch64-apple-darwin) printf '%s\n' --mac --arm64 ;;
    x86_64-apple-darwin) printf '%s\n' --mac --x64 ;;
    aarch64-unknown-linux-gnu) printf '%s\n' --linux --arm64 ;;
    x86_64-unknown-linux-gnu) printf '%s\n' --linux --x64 ;;
    x86_64-pc-windows-msvc) printf '%s\n' --win --x64 ;;
    *)
      echo "Unsupported Electron target triple: $TARGET" >&2
      exit 2
      ;;
  esac
}

check_signing_prereqs

if [ "$SKIP_FETCH" -eq 0 ]; then
  fetch_args=()
  if [ -n "$TARGET" ]; then
    fetch_args+=(--target "$TARGET")
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    fetch_args+=(--dry-run)
  fi
  run "$ROOT/scripts/fetch-binaries.sh" "${fetch_args[@]}"
fi

run pnpm --filter @cerul/desktop build
cargo_args=(build -p cerul-api --release)
if [ -n "$TARGET" ]; then
  cargo_args+=(--target "$TARGET")
fi
run cargo "${cargo_args[@]}"
if [ -n "$TARGET" ]; then
  run env CERUL_TARGET_TRIPLE="$TARGET" pnpm --filter @cerul/electron-shell stage:cerul-api
else
  run pnpm --filter @cerul/electron-shell stage:cerul-api
fi
run pnpm --filter @cerul/electron-shell build

builder_args=(--publish never)
while IFS= read -r arg; do
  [ -n "$arg" ] && builder_args+=("$arg")
done < <(electron_builder_args)

if [ "$NO_BUNDLE" -eq 1 ] || [ "$DEBUG" -eq 1 ]; then
  builder_args+=(--dir)
fi

run pnpm --filter @cerul/electron-shell exec electron-builder "${builder_args[@]}"

if [ "$DRY_RUN" -eq 1 ]; then
  exit 0
fi

bundle_root="$ROOT/target/electron"
if [ ! -d "$bundle_root" ]; then
  echo "Electron output directory was not created: $bundle_root" >&2
  exit 1
fi

if [ "$NO_BUNDLE" -eq 1 ] || [ "$DEBUG" -eq 1 ] || [ "$DRY_RUN" -eq 1 ]; then
  exit 0
fi

echo "Installer artifacts:"
artifacts="$(find "$bundle_root" -type f \( -name "*.dmg" -o -name "*.zip" -o -name "*.msi" -o -name "*.exe" -o -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" \) -print 2>/dev/null || true)"
if [ -z "$artifacts" ]; then
  echo "No installer artifacts found under $bundle_root." >&2
  exit 1
fi
printf '%s\n' "$artifacts"
