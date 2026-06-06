#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM=""
PROFILE="release"
TARGET=""
BUNDLE_ROOT=""
MAX_INSTALLER_MIB="${CERUL_MAX_INSTALLER_MIB:-120}"
MODELS_CACHE=""
MODELS_CACHE_ONLY=0
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/smoke-release-footprint.sh [--platform <macos|linux|windows>] [--profile <release|debug>] [--target <triple>] [--bundle-root <path>] [--max-installer-mib <n>] [--models-cache <path>] [--models-cache-only] [--dry-run]

Records release footprint evidence:

  - installer artifact byte sizes for the current platform
  - optional hard fail when any installer exceeds --max-installer-mib
  - first-run model download estimates for Whisper, Qwen3-VL, and Fast mode
  - optional actual model cache size when --models-cache is provided

Set --max-installer-mib 0 to report installer sizes without enforcing a budget.
Set --models-cache-only to record model estimates/cache size without requiring
installer artifacts.
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
    --max-installer-mib)
      MAX_INSTALLER_MIB="${2:?missing max installer MiB}"
      shift 2
      ;;
    --models-cache)
      MODELS_CACHE="${2:?missing models cache path}"
      shift 2
      ;;
    --models-cache-only)
      MODELS_CACHE_ONLY=1
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

case "$MAX_INSTALLER_MIB" in
  ''|*[!0-9]*)
    echo "--max-installer-mib must be a non-negative integer." >&2
    exit 2
    ;;
esac

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
    echo "Unsupported platform for release footprint smoke: $PLATFORM" >&2
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
  if [ "$MODELS_CACHE_ONLY" -eq 0 ]; then
    echo "+ inspect installer footprint under $BUNDLE_ROOT for platform=$PLATFORM max_installer_mib=$MAX_INSTALLER_MIB"
  fi
  echo "+ report first-run model download estimates"
  if [ -n "$MODELS_CACHE" ]; then
    echo "+ measure actual model cache size at $MODELS_CACHE"
  fi
  exit 0
fi

if [ "$MODELS_CACHE_ONLY" -eq 0 ] && [ ! -d "$BUNDLE_ROOT" ]; then
  echo "Bundle output directory was not created: $BUNDLE_ROOT" >&2
  exit 1
fi

file_size_bytes() {
  local file="$1"
  if stat -f '%z' "$file" >/dev/null 2>&1; then
    stat -f '%z' "$file"
  elif stat -c '%s' "$file" >/dev/null 2>&1; then
    stat -c '%s' "$file"
  else
    wc -c <"$file" | tr -d ' '
  fi
}

bytes_to_mib() {
  awk -v bytes="$1" 'BEGIN { printf "%.1f", bytes / 1048576 }'
}

find_platform_artifacts() {
  case "$PLATFORM" in
    macos)
      find "$BUNDLE_ROOT" -type f -name "*.dmg" -size +0c -print 2>/dev/null | sort
      ;;
    linux)
      find "$BUNDLE_ROOT" -type f \( -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" \) -size +0c -print 2>/dev/null | sort
      ;;
    windows)
      find "$BUNDLE_ROOT" -type f \( -name "*.msi" -o -name "*.exe" \) \
        ! -path "*/win-unpacked/*" \
        ! -path "*/resources/*" \
        -size +0c -print 2>/dev/null | sort
      ;;
  esac
}

budget_bytes=$((MAX_INSTALLER_MIB * 1024 * 1024))
artifact_count=0
if [ "$MODELS_CACHE_ONLY" -eq 0 ]; then
  artifacts="$(find_platform_artifacts)"
  if [ -z "$artifacts" ]; then
    echo "No non-empty installer artifacts found for platform=$PLATFORM under $BUNDLE_ROOT." >&2
    exit 1
  fi

  while IFS= read -r artifact; do
    [ -n "$artifact" ] || continue
    artifact_count=$((artifact_count + 1))
    bytes="$(file_size_bytes "$artifact")"
    mib="$(bytes_to_mib "$bytes")"
    echo "release_footprint_artifact platform=$PLATFORM path=$artifact bytes=$bytes mib=$mib max_mib=$MAX_INSTALLER_MIB"
    if [ "$MAX_INSTALLER_MIB" -gt 0 ] && [ "$bytes" -gt "$budget_bytes" ]; then
      echo "Installer artifact exceeds budget: $artifact is ${mib}MiB, max is ${MAX_INSTALLER_MIB}MiB." >&2
      exit 1
    fi
  done <<EOF
$artifacts
EOF
fi

echo "release_footprint_model_estimate name=whisper-base.en mib=142 source=api-model-catalog"
echo "release_footprint_model_estimate name=whisper-small.en mib=466 source=api-model-catalog"
echo "release_footprint_model_estimate name=whisper-large-v3 mib=2969 source=api-model-catalog"
echo "release_footprint_model_estimate name=qwen3-vl-embedding-2b mib=4096 source=design-doc"
echo "release_footprint_model_estimate name=local-qwen3-vl repos=mlx-community/Qwen3-VL-Embedding-2B-6bit source=embed-config"

if [ -n "$MODELS_CACHE" ]; then
  if [ ! -d "$MODELS_CACHE" ]; then
    echo "Model cache directory not found: $MODELS_CACHE" >&2
    exit 1
  fi
  cache_kib="$(du -sk "$MODELS_CACHE" | awk '{print $1}')"
  cache_bytes=$((cache_kib * 1024))
  cache_mib="$(bytes_to_mib "$cache_bytes")"
  echo "release_footprint_model_cache path=$MODELS_CACHE bytes=$cache_bytes mib=$cache_mib"
fi

echo "release_footprint_smoke platform=$PLATFORM artifact_count=$artifact_count max_installer_mib=$MAX_INSTALLER_MIB models_cache_only=$MODELS_CACHE_ONLY"
