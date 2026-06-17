#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM=""
PROFILE="release"
TARGET=""
BUNDLE_ROOT=""
DRY_RUN=0
DIR_ONLY=0
MACOS_UPDATE_ONLY=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-release-artifacts.sh [--platform <macos|linux|windows>] [--profile <release|debug>] [--target <triple>] [--bundle-root <path>] [--dir-only] [--macos-update-only] [--dry-run]

Checks that the current platform produced the expected Electron release artifacts:

  macos   Cerul.app plus at least one non-empty .dmg, or update zip metadata with --macos-update-only
  linux   at least one non-empty .AppImage, .deb, or .rpm
  windows at least one non-empty .msi or .exe

The check is intentionally shallow: it catches missing or empty release
artifacts on all platforms, while deeper installed-app behavior remains covered
by platform-specific smokes.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform)
      PLATFORM="${2:?missing platform}"
      shift 2
      ;;
    --profile)
      PROFILE="${2:?missing profile}"
      shift 2
      ;;
    --target)
      TARGET="${2:?missing target triple}"
      shift 2
      ;;
    --bundle-root)
      BUNDLE_ROOT="${2:?missing bundle root}"
      shift 2
      ;;
    --dir-only)
      DIR_ONLY=1
      shift
      ;;
    --macos-update-only)
      MACOS_UPDATE_ONLY=1
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

host_platform() {
  case "$(uname -s)" in
    Darwin) echo "macos" ;;
    Linux) echo "linux" ;;
    MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
    *) echo "unknown" ;;
  esac
}

PLATFORM="${PLATFORM:-$(host_platform)}"
case "$PLATFORM" in
  macos|linux|windows)
    ;;
  *)
    echo "Unsupported platform for release artifact smoke: $PLATFORM" >&2
    exit 2
    ;;
esac

case "$PROFILE" in
  release|debug)
    ;;
  *)
    echo "--profile must be release or debug." >&2
    exit 2
    ;;
esac

if [ -z "$BUNDLE_ROOT" ]; then
  BUNDLE_ROOT="$ROOT/target/electron"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ inspect $PLATFORM Electron artifacts under $BUNDLE_ROOT dir_only=$DIR_ONLY macos_update_only=$MACOS_UPDATE_ONLY"
  exit 0
fi

if [ ! -d "$BUNDLE_ROOT" ]; then
  echo "Bundle output directory was not created: $BUNDLE_ROOT" >&2
  exit 1
fi

non_empty_files() {
  find "$BUNDLE_ROOT" -type f "$@" -size +0c -print 2>/dev/null | sort
}

require_any_artifact() {
  local label="$1"
  shift
  local matches
  matches="$(non_empty_files "$@")"
  if [ -z "$matches" ]; then
    echo "No non-empty $label artifacts found under $BUNDLE_ROOT." >&2
    exit 1
  fi
  printf '%s\n' "$matches"
}

artifact_count=0

case "$PLATFORM" in
  macos)
    app_path="$(find "$BUNDLE_ROOT" -type d -name "Cerul.app" -print -quit 2>/dev/null || true)"
    if [ -z "$app_path" ]; then
      echo "No Cerul.app bundle found under $BUNDLE_ROOT." >&2
      exit 1
    fi

    if [ ! -f "$app_path/Contents/Info.plist" ]; then
      echo "Cerul.app is missing Contents/Info.plist." >&2
      exit 1
    fi

    bin_path="$(find "$app_path/Contents/MacOS" -maxdepth 1 -type f -perm -111 -print -quit 2>/dev/null || true)"
    if [ -z "$bin_path" ]; then
      echo "Cerul.app does not contain an executable in Contents/MacOS." >&2
      exit 1
    fi

    api_path="$app_path/Contents/Resources/bin/cerul-core"
    if [ ! -x "$api_path" ]; then
      echo "Cerul.app is missing executable packaged Cerul Core at $api_path." >&2
      exit 1
    fi

    if [ "$DIR_ONLY" -eq 1 ]; then
      echo "release_artifact_smoke platform=macos app=$app_path executable=$bin_path api=$api_path dir_only=true"
      artifacts="$app_path"
    elif [ "$MACOS_UPDATE_ONLY" -eq 1 ]; then
      zip_artifacts="$(require_any_artifact "macOS update ZIP" -name "*.zip")"
      yml_artifacts="$(require_any_artifact "macOS update metadata" -name "latest-mac.yml")"
      blockmap_artifacts="$(require_any_artifact "macOS update blockmap" -name "*.zip.blockmap")"
      artifact_count="$(printf '%s\n%s\n%s\n' "$zip_artifacts" "$yml_artifacts" "$blockmap_artifacts" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
      echo "release_artifact_smoke platform=macos app=$app_path executable=$bin_path api=$api_path update_only=true artifact_count=$artifact_count"
      artifacts="$(printf '%s\n%s\n%s\n' "$zip_artifacts" "$yml_artifacts" "$blockmap_artifacts")"
    else
      artifacts="$(require_any_artifact "macOS DMG" -name "*.dmg")"
      artifact_count="$(printf '%s\n' "$artifacts" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
      echo "release_artifact_smoke platform=macos app=$app_path executable=$bin_path api=$api_path dmg_count=$artifact_count"
    fi
    ;;
  linux)
    if [ "$DIR_ONLY" -eq 1 ]; then
      unpacked="$(find "$BUNDLE_ROOT" -type d -name "linux-unpacked" -print -quit 2>/dev/null || true)"
      if [ -z "$unpacked" ]; then
        echo "No linux-unpacked directory found under $BUNDLE_ROOT." >&2
        exit 1
      fi
      api_path="$unpacked/resources/bin/cerul-core"
      if [ ! -x "$api_path" ]; then
        echo "linux-unpacked is missing executable packaged Cerul Core at $api_path." >&2
        exit 1
      fi
      artifacts="$unpacked"
      echo "release_artifact_smoke platform=linux unpacked=$unpacked api=$api_path dir_only=true"
    else
      artifacts="$(require_any_artifact "Linux installer" \( -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" \))"
      artifact_count="$(printf '%s\n' "$artifacts" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
      echo "release_artifact_smoke platform=linux artifact_count=$artifact_count"
    fi
    ;;
  windows)
    if [ "$DIR_ONLY" -eq 1 ]; then
      unpacked="$(find "$BUNDLE_ROOT" -type d -name "win-unpacked" -print -quit 2>/dev/null || true)"
      if [ -z "$unpacked" ]; then
        echo "No win-unpacked directory found under $BUNDLE_ROOT." >&2
        exit 1
      fi
      api_path="$unpacked/resources/bin/cerul-core.exe"
      if [ ! -f "$api_path" ]; then
        echo "win-unpacked is missing packaged Cerul Core at $api_path." >&2
        exit 1
      fi
      artifacts="$unpacked"
      echo "release_artifact_smoke platform=windows unpacked=$unpacked api=$api_path dir_only=true"
    else
      artifacts="$(require_any_artifact "Windows installer" \( -name "*.msi" -o -name "*.exe" \) ! -path "*/win-unpacked/*" ! -path "*/resources/*")"
      artifact_count="$(printf '%s\n' "$artifacts" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
      echo "release_artifact_smoke platform=windows artifact_count=$artifact_count"
    fi
    ;;
esac

printf '%s\n' "$artifacts"
