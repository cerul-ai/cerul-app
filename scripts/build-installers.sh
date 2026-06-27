#!/usr/bin/env bash
set -euo pipefail

# Installer builds enforce pinned third-party binaries (see fetch-binaries.sh).
export CERUL_RELEASE_BUILD=1

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/corepack-pnpm.sh"
TARGET=""
DEBUG=0
NO_BUNDLE=0
SKIP_FETCH=0
DRY_RUN=0
REQUIRE_SIGNING=0
REBUILD_MLX=0
MAC_TARGETS=""
PREPACKAGED_APP=""

usage() {
  cat <<'EOF'
Usage: scripts/build-installers.sh [--target <triple>] [--mac-targets <targets>] [--prepackaged-app <path>] [--debug] [--no-bundle] [--skip-fetch] [--require-signing] [--dry-run]

Builds Cerul installers with Electron. The build contract is:
  1. build the React renderer
  2. build release Cerul Core
  3. stage Cerul Core into apps/electron-shell/bin/
  4. run electron-builder, which copies bin/, desktop-dist/, third-party/, and mlx-sidecar/

Pass --mac-targets zip for fast auto-update artifacts or --mac-targets dmg
to build only the DMG from a prepackaged app bundle.

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
    --rebuild-mlx-runtime)
      REBUILD_MLX=1
      shift
      ;;
    --mac-targets)
      MAC_TARGETS="${2:?missing mac targets}"
      shift 2
      ;;
    --prepackaged-app)
      PREPACKAGED_APP="${2:?missing prepackaged app path}"
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

create_mlx_runtime_archive() {
  run rm -f "$MLX_RUNTIME_ARCHIVE" "$MLX_RUNTIME_ARCHIVE.sha256"
  run python3 - "$MLX_RUNTIME_DIR" "$MLX_RUNTIME_ARCHIVE" <<'PY'
import gzip
import os
import sys
import tarfile

source, destination = sys.argv[1], sys.argv[2]
with open(destination, "wb") as raw:
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gzip_file:
        with tarfile.open(fileobj=gzip_file, mode="w") as archive:
            for root, dirs, files in os.walk(source):
                dirs.sort()
                files.sort()
                for name in [*dirs, *files]:
                    path = os.path.join(root, name)
                    arcname = os.path.relpath(path, source)
                    info = archive.gettarinfo(path, arcname=arcname)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 0
                    if info.isfile():
                        with open(path, "rb") as file:
                            archive.addfile(info, file)
                    else:
                        archive.addfile(info)
PY
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+ shasum -a 256 %q > %q\n' "$MLX_RUNTIME_ARCHIVE" "$MLX_RUNTIME_ARCHIVE.sha256"
  else
    shasum -a 256 "$MLX_RUNTIME_ARCHIVE" | awk '{print $1}' > "$MLX_RUNTIME_ARCHIVE.sha256"
  fi
}

create_mlx_runtime_manifest() {
  local digest
  local size
  local archive_name
  local base_url
  digest="$(tr -d '[:space:]' < "$MLX_RUNTIME_ARCHIVE.sha256")"
  if [ -z "$digest" ]; then
    echo "MLX runtime sha256 is empty." >&2
    exit 1
  fi
  if [ "$(uname -s)" = "Darwin" ]; then
    size="$(stat -f%z "$MLX_RUNTIME_ARCHIVE")"
  else
    size="$(stat -c%s "$MLX_RUNTIME_ARCHIVE")"
  fi
  archive_name="mlx-runtime-darwin-arm64-$digest.tar.gz"
  base_url="${CERUL_MLX_RUNTIME_BASE_URL:-https://updates.cerul.ai/runtime}"
  base_url="${base_url%/}"
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+ write %q for %q\n' "$MLX_RUNTIME_MANIFEST" "$archive_name"
    return
  fi
  python3 - "$MLX_RUNTIME_MANIFEST" "$archive_name" "$base_url/$archive_name" "$digest" "$size" <<'PY'
import json
import sys

destination, archive, url, sha256, size = sys.argv[1:]
with open(destination, "w", encoding="utf-8") as file:
    json.dump(
        {
            "platform": "darwin-arm64",
            "archive": archive,
            "url": url,
            "sha256": sha256,
            "size": int(size),
        },
        file,
        indent=2,
        sort_keys=True,
    )
    file.write("\n")
PY
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

require_command() {
  local name="$1"

  if ! command -v "$name" >/dev/null 2>&1; then
    echo "$name is required when --require-signing is used." >&2
    exit 2
  fi
}

electron_builder_identity_name() {
  local identity="$1"
  case "$identity" in
    "Developer ID Application:"*) identity="${identity#Developer ID Application:}" ;;
    "Developer ID Installer:"*) identity="${identity#Developer ID Installer:}" ;;
    "3rd Party Mac Developer Application:"*) identity="${identity#3rd Party Mac Developer Application:}" ;;
    "3rd Party Mac Developer Installer:"*) identity="${identity#3rd Party Mac Developer Installer:}" ;;
  esac
  identity="${identity#"${identity%%[![:space:]]*}"}"
  identity="${identity%"${identity##*[![:space:]]}"}"
  printf '%s\n' "$identity"
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
  export CSC_NAME
  CSC_NAME="$(electron_builder_identity_name "${CSC_NAME:-$APPLE_SIGNING_IDENTITY}")"
  export APPLE_APP_SPECIFIC_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD:-$APPLE_PASSWORD}"
}

has_zvec_runtime_override() {
  [ -n "${ZVEC_ROOT:-}" ] ||
    [ -n "${ZVEC_LIB_DIR:-}" ] ||
    [ -n "${ZVEC_BUNDLED_WHEEL_PATH:-}" ] ||
    { [ -n "${ZVEC_BUNDLED_WHEEL_URL:-}" ] && [ -n "${ZVEC_BUNDLED_WHEEL_SHA256:-}" ]; }
}

check_zvec_target_prereqs() {
  if [ -n "$PREPACKAGED_APP" ]; then
    return
  fi

  local effective_target="$TARGET"
  if [ -z "$effective_target" ] && [ "$(uname -s)" = "Darwin" ]; then
    case "$(uname -m)" in
      arm64) effective_target="aarch64-apple-darwin" ;;
      x86_64) effective_target="x86_64-apple-darwin" ;;
    esac
  fi

  case "$effective_target" in
    x86_64-apple-darwin)
      if has_zvec_runtime_override; then
        return
      fi
      cat >&2 <<'EOF'
x86_64-apple-darwin is not supported by zvec's bundled runtime wheels.

To build Intel macOS artifacts, provide a matching zvec runtime through
ZVEC_ROOT/ZVEC_LIB_DIR or ZVEC_BUNDLED_WHEEL_PATH. Otherwise build the
supported Apple Silicon target with --target aarch64-apple-darwin.
EOF
      exit 2
      ;;
    x86_64-pc-windows-msvc)
      if has_zvec_runtime_override; then
        return
      fi
      cat >&2 <<'EOF'
x86_64-pc-windows-msvc requires an explicit zvec runtime override.

To build Windows artifacts, provide a matching zvec runtime through
ZVEC_ROOT/ZVEC_LIB_DIR or ZVEC_BUNDLED_WHEEL_PATH.
EOF
      exit 2
      ;;
  esac
}

electron_builder_args() {
  if [ -z "$TARGET" ]; then
    return
  fi

  case "$TARGET" in
    aarch64-apple-darwin)
      if [ -n "$MAC_TARGETS" ]; then
        printf '%s\n' --arm64
      else
        printf '%s\n' --mac --arm64
      fi
      ;;
    x86_64-apple-darwin)
      if [ -n "$MAC_TARGETS" ]; then
        printf '%s\n' --x64
      else
        printf '%s\n' --mac --x64
      fi
      ;;
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
check_zvec_target_prereqs
prepare_corepack_pnpm_path "$ROOT" "$DRY_RUN"
if [ -z "${APPLE_APP_SPECIFIC_PASSWORD:-}" ] && [ -n "${APPLE_PASSWORD:-}" ]; then
  export APPLE_APP_SPECIFIC_PASSWORD="$APPLE_PASSWORD"
fi
if target_is_macos &&
  [ -z "${CSC_LINK:-}" ] &&
  [ -z "${CSC_NAME:-}" ] &&
  [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  export CSC_IDENTITY_AUTO_DISCOVERY="${CSC_IDENTITY_AUTO_DISCOVERY:-false}"
fi

if [ -n "$PREPACKAGED_APP" ] && [ ! -d "$PREPACKAGED_APP" ] && [ "$DRY_RUN" -eq 0 ]; then
  echo "Prepackaged app was not found: $PREPACKAGED_APP" >&2
  exit 1
fi

if [ -n "$PREPACKAGED_APP" ] && [ "$DRY_RUN" -eq 0 ]; then
  prepackaged_parent="$(cd "$(dirname "$PREPACKAGED_APP")" && pwd -P)"
  prepackaged_abs="$prepackaged_parent/$(basename "$PREPACKAGED_APP")"
  mkdir -p "$ROOT/target/electron"
  electron_output_abs="$(cd "$ROOT/target/electron" && pwd -P)"
  case "$prepackaged_abs" in
    "$electron_output_abs"/*)
      echo "Prepackaged app must be outside $electron_output_abs because electron-builder cleans its output directory." >&2
      exit 2
      ;;
  esac
fi

if [ "$SKIP_FETCH" -eq 0 ] && [ -z "$PREPACKAGED_APP" ]; then
  fetch_args=()
  if [ -n "$TARGET" ]; then
    fetch_args+=(--target "$TARGET")
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    fetch_args+=(--dry-run)
  fi
  time_step fetch_binaries run "$ROOT/scripts/fetch-binaries.sh" ${fetch_args[@]+"${fetch_args[@]}"}
fi

if [ -z "$PREPACKAGED_APP" ]; then
  time_step react_build run pnpm --filter @cerul/desktop build
  cargo_args=(build -p cerul-api --release)
  if [ -n "$TARGET" ]; then
    cargo_args+=(--target "$TARGET")
  fi
  time_step cargo_release_build run cargo "${cargo_args[@]}"
  if [ -n "$TARGET" ]; then
    time_step stage_cerul_core run env CERUL_TARGET_TRIPLE="$TARGET" pnpm --filter @cerul/electron-shell stage:cerul-core
  else
    time_step stage_cerul_core run pnpm --filter @cerul/electron-shell stage:cerul-core
  fi
  time_step electron_main_build run pnpm --filter @cerul/electron-shell build
else
  echo "Using prepackaged app bundle: $PREPACKAGED_APP"
fi

# Bundled on-device MLX Python runtime (macOS only). Reuse an existing build
# unless --rebuild-mlx-runtime is passed; always ensure the directory exists so
# electron-builder's extraResources archive never fails on other platforms.
MLX_RUNTIME_DIR="$ROOT/apps/electron-shell/mlx-runtime"
MLX_RUNTIME_ARCHIVE="$ROOT/apps/electron-shell/mlx-runtime.tar.gz"
MLX_RUNTIME_MANIFEST="$ROOT/apps/electron-shell/mlx-runtime-manifest.json"
mkdir -p "$MLX_RUNTIME_DIR"
if target_is_macos; then
  if [ -z "$PREPACKAGED_APP" ]; then
    if [ "$REBUILD_MLX" -eq 1 ] || [ ! -x "$MLX_RUNTIME_DIR/bin/python3" ]; then
      time_step mlx_runtime_build run "$ROOT/scripts/build-mlx-runtime.sh"
    else
      echo "Reusing existing MLX runtime at $MLX_RUNTIME_DIR (pass --rebuild-mlx-runtime to force a fresh build)."
    fi
    if [ -x "$MLX_RUNTIME_DIR/bin/python3" ] && [ -n "${APPLE_SIGNING_IDENTITY:-${CSC_NAME:-}}" ]; then
      sign_args=(--runtime-dir "$MLX_RUNTIME_DIR" --identity "${APPLE_SIGNING_IDENTITY:-$CSC_NAME}")
      if [ -n "${APPLE_TEAM_ID:-}" ]; then
        sign_args+=(--expected-team-id "$APPLE_TEAM_ID")
      fi
      time_step mlx_runtime_signing run node "$ROOT/apps/electron-shell/scripts/sign-mlx-runtime.cjs" "${sign_args[@]}"
    fi
  fi
fi
if [ -z "$PREPACKAGED_APP" ]; then
  time_step mlx_runtime_archive create_mlx_runtime_archive
  time_step mlx_runtime_manifest create_mlx_runtime_manifest
fi

builder_args=(--publish never)
if target_is_macos && [ -n "$MAC_TARGETS" ]; then
  builder_args+=(--mac)
  IFS=',' read -r -a mac_targets_array <<< "$MAC_TARGETS"
  for mac_target in "${mac_targets_array[@]}"; do
    mac_target="${mac_target#"${mac_target%%[![:space:]]*}"}"
    mac_target="${mac_target%"${mac_target##*[![:space:]]}"}"
    [ -n "$mac_target" ] && builder_args+=("$mac_target")
  done
fi
while IFS= read -r arg; do
  [ -n "$arg" ] && builder_args+=("$arg")
done < <(electron_builder_args)

if [ -n "$PREPACKAGED_APP" ]; then
  builder_args+=(--prepackaged "$PREPACKAGED_APP")
fi

if [ "$NO_BUNDLE" -eq 1 ] || [ "$DEBUG" -eq 1 ]; then
  builder_args+=(--dir)
fi

bundle_root="$ROOT/target/electron"
preserved_update_assets_dir=""
if target_is_macos &&
  [ "$MAC_TARGETS" = "dmg" ] &&
  [ -n "$PREPACKAGED_APP" ] &&
  [ "$DRY_RUN" -eq 0 ] &&
  [ -d "$bundle_root" ]; then
  preserved_update_assets_dir="$(mktemp -d)"
  while IFS= read -r artifact; do
    cp "$artifact" "$preserved_update_assets_dir/"
  done < <(find "$bundle_root" -maxdepth 1 -type f \( -name "*.zip" -o -name "*.zip.blockmap" -o -name "latest-mac.yml" \) -print)
fi

builder_step="electron_builder"
if target_is_macos && [ "$MAC_TARGETS" = "dmg" ]; then
  builder_step="dmg_build"
fi
time_step "$builder_step" run pnpm --filter @cerul/electron-shell exec electron-builder "${builder_args[@]}"

if [ -n "$preserved_update_assets_dir" ]; then
  mkdir -p "$bundle_root"
  while IFS= read -r artifact; do
    cp "$artifact" "$bundle_root/"
  done < <(find "$preserved_update_assets_dir" -maxdepth 1 -type f -print)
  rm -rf "$preserved_update_assets_dir"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  exit 0
fi

if [ ! -d "$bundle_root" ]; then
  echo "Electron output directory was not created: $bundle_root" >&2
  exit 1
fi

if [ "$NO_BUNDLE" -eq 1 ] || [ "$DEBUG" -eq 1 ] || [ "$DRY_RUN" -eq 1 ]; then
  exit 0
fi

if target_is_macos &&
  [ -n "${APPLE_SIGNING_IDENTITY:-${CSC_NAME:-}}" ] &&
  [ "${CERUL_NOTARIZE:-0}" = "1" ] &&
  find "$bundle_root" -maxdepth 1 -type f -name "*.dmg" -print -quit | grep -q .; then
  time_step macos_release_finalize run "$ROOT/scripts/finalize-macos-release-artifacts.sh" --bundle-root "$bundle_root"
fi

echo "Installer artifacts:"
artifacts="$(find "$bundle_root" -maxdepth 1 -type f \( -name "*.dmg" -o -name "*.zip" -o -name "*.msi" -o -name "*.exe" -o -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" \) -print 2>/dev/null || true)"
if [ -z "$artifacts" ]; then
  echo "No installer artifacts found under $bundle_root." >&2
  exit 1
fi
printf '%s\n' "$artifacts"
