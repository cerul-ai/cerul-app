#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=""
DRY_RUN=0
FORCE=0

usage() {
  cat <<'EOF'
Usage: scripts/fetch-binaries.sh [--target <triple>] [--force] [--dry-run]

Stages ffmpeg, yt-dlp, and qdrant into third-party/<target-triple>/ for desktop packaging.

ffmpeg is copied from PATH unless CERUL_FFMPEG_URL or a target-specific
CERUL_FFMPEG_URL_<TARGET> is set. On macOS, non-system dynamic libraries are
copied next to the staged ffmpeg binary and rewritten to relative load paths.
The URL may point to a binary, .zip, .tar.gz, or .tar.xz archive containing an
ffmpeg executable.

qdrant is downloaded from official Qdrant GitHub releases unless
CERUL_QDRANT_DOWNLOAD_URL or a target-specific
CERUL_QDRANT_DOWNLOAD_URL_<TARGET> is set.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      TARGET="${2:?missing target}"
      shift 2
      ;;
    --force)
      FORCE=1
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

host_target() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "unsupported" ;;
  esac
}

HOST_TARGET="$(host_target)"
TARGET="${TARGET:-$HOST_TARGET}"
if [ "$TARGET" = "unsupported" ]; then
  echo "Cannot infer target triple for this host; pass --target explicitly." >&2
  exit 2
fi

target_os() {
  case "$1" in
    *apple-darwin) echo "macos" ;;
    *unknown-linux-gnu|*unknown-linux-musl) echo "linux" ;;
    *pc-windows-msvc) echo "windows" ;;
    *) echo "unknown" ;;
  esac
}

target_arch() {
  case "$1" in
    aarch64-*) echo "aarch64" ;;
    x86_64-*) echo "x86_64" ;;
    *) echo "unknown" ;;
  esac
}

exe_suffix() {
  if [ "$(target_os "$TARGET")" = "windows" ]; then
    echo ".exe"
  fi
}

sanitize_env_target() {
  printf '%s' "$1" | tr '[:lower:]-' '[:upper:]_'
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

# Pinned third-party binary releases. These flow straight into the packaged
# installers via extraResources, so they must be reproducible and verified —
# `releases/latest` plus no checksum meant any compromised upstream release
# would ship to users unnoticed.
YTDLP_VERSION="${CERUL_YTDLP_VERSION:-2026.06.09}"
QDRANT_VERSION="${CERUL_QDRANT_VERSION:-v1.18.2}"
# Cerul-vendored, self-contained LGPL ffmpeg (built from official source with no
# --enable-gpl / x264, hosted on the cerul-app releases). See ffmpeg_url().
FFMPEG_VERSION="${CERUL_FFMPEG_VERSION:-7.1}"

# sha256 per pinned asset. Update together with the versions above.
expected_sha256() {
  case "$1" in
    yt-dlp_macos) echo "b82c3626952e6c14eaf654cc565866775ffd0b9ffb7021628ac59b42c2f4f244" ;;
    yt-dlp_linux) echo "bf8aac79b72287a6d2043074415132558b43743a8f9461a22b0141e90f16ce66" ;;
    yt-dlp_linux_aarch64) echo "cabd246445bdfde0eda0dfe68bbe90354be83f3fdbbf077df11a2ea55f41cdbd" ;;
    yt-dlp.exe) echo "3a48cb955d55c8821b60ccbdbbc6f61bc958f2f3d3b7ad5eaf3d83a543293a27" ;;
    qdrant-aarch64-apple-darwin.tar.gz) echo "859f487e316ae1bda3b5d7c1e129a0a7344424d992503c188979ca6ac1b47253" ;;
    qdrant-x86_64-apple-darwin.tar.gz) echo "d395eb3d96c2196bbb8c611b800842928fb8b4997924b585bf42ce0ceb90fa1f" ;;
    qdrant-aarch64-unknown-linux-musl.tar.gz) echo "2ead5bb8206289b67c930f0eb29123228ddb43c2344551a0947cbc9046f92c6c" ;;
    qdrant-x86_64-unknown-linux-musl.tar.gz) echo "40a6af44f8a496560c9d2352b6b2a0ada816aa48d0781c68f602582e67b3aea0" ;;
    qdrant-x86_64-pc-windows-msvc.zip) echo "b2b262cba6f78cf4fa794ae78d73a8f70a221c93c76c75ac8fd6fe95d809b142" ;;
    ffmpeg-7.1-lgpl-macos-arm64.tar.gz) echo "157076bb3e83f31e7a39781200173eb730edafed9481ed5c5a3b3a2adee416fa" ;;
    ffmpeg-7.1-lgpl-macos-x86_64.tar.gz) echo "a13c65f9986d970bb89eee172959aa5c6b09534e8c045575eeba1cdab444fd86" ;;
    *) return 1 ;;
  esac
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

verify_download_checksum() {
  local file="$1"
  local url="$2"
  [ "$DRY_RUN" -eq 1 ] && return 0
  local asset expected actual
  asset="$(basename "$url")"
  if ! expected="$(expected_sha256 "$asset")"; then
    # Assets supplied via override URLs have no pinned checksum; allow them
    # but make the gap visible in the build log.
    echo "WARNING: no pinned sha256 for $asset; skipping verification" >&2
    return 0
  fi
  actual="$(sha256_of "$file")"
  if [ "$actual" != "$expected" ]; then
    echo "Checksum mismatch for $asset" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    return 1
  fi
}

download() {
  local url="$1"
  local out="$2"
  if [ "$DRY_RUN" -eq 1 ]; then
    run curl -fL --retry 3 --retry-delay 2 -o "$out" "$url"
    return 0
  fi
  curl -fL --retry 3 --retry-delay 2 -o "$out" "$url"
}

stage_from_archive() {
  local url="$1"
  local executable="$2"
  local dest="$3"
  local tmp
  tmp="$(mktemp -d)"
  local archive="$tmp/download"

  if ! download "$url" "$archive"; then
    rm -rf "$tmp"
    return 1
  fi
  if ! verify_download_checksum "$archive" "$url"; then
    rm -rf "$tmp"
    exit 1
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    rm -rf "$tmp"
    return
  fi

  case "$url" in
    *.zip) unzip -q "$archive" -d "$tmp/unpacked" ;;
    *.tar.gz|*.tgz) mkdir -p "$tmp/unpacked"; tar -xzf "$archive" -C "$tmp/unpacked" ;;
    *.tar.xz|*.txz) mkdir -p "$tmp/unpacked"; tar -xJf "$archive" -C "$tmp/unpacked" ;;
    *)
      cp "$archive" "$dest"
      chmod 0755 "$dest"
      rm -rf "$tmp"
      return
      ;;
  esac

  local found
  found="$(find "$tmp/unpacked" -type f -name "$executable" -print -quit)"
  if [ -z "$found" ]; then
    echo "Archive did not contain $executable: $url" >&2
    exit 1
  fi
  cp "$found" "$dest"
  chmod 0755 "$dest"
  rm -rf "$tmp"
}

target_specific_ffmpeg_url() {
  local env_name="CERUL_FFMPEG_URL_$(sanitize_env_target "$TARGET")"
  printf '%s' "${!env_name:-${CERUL_FFMPEG_URL:-}}"
}

ffmpeg_url() {
  local override
  override="$(target_specific_ffmpeg_url)"
  if [ -n "$override" ]; then
    echo "$override"
    return 0
  fi

  # Default: Cerul-vendored, self-contained LGPL ffmpeg built from official
  # source (no --enable-gpl, no x264) and hosted on the cerul-app releases, so
  # release installers never ship a GPL/system ffmpeg. Checksum-pinned above.
  case "$(target_os "$TARGET")-$(target_arch "$TARGET")" in
    macos-aarch64) echo "https://github.com/cerul-ai/cerul-app/releases/download/ffmpeg-vendor-${FFMPEG_VERSION}-lgpl/ffmpeg-${FFMPEG_VERSION}-lgpl-macos-arm64.tar.gz" ;;
    macos-x86_64) echo "https://github.com/cerul-ai/cerul-app/releases/download/ffmpeg-vendor-${FFMPEG_VERSION}-lgpl/ffmpeg-${FFMPEG_VERSION}-lgpl-macos-x86_64.tar.gz" ;;
    *) return 1 ;;
  esac
}

target_specific_qdrant_url() {
  local env_name="CERUL_QDRANT_DOWNLOAD_URL_$(sanitize_env_target "$TARGET")"
  printf '%s' "${!env_name:-${CERUL_QDRANT_DOWNLOAD_URL:-}}"
}

ytdlp_url() {
  case "$(target_os "$TARGET")-$(target_arch "$TARGET")" in
    macos-*) echo "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_macos" ;;
    linux-aarch64) echo "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux_aarch64" ;;
    linux-x86_64) echo "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp_linux" ;;
    windows-*) echo "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/yt-dlp.exe" ;;
    *) return 1 ;;
  esac
}

qdrant_url() {
  local override
  override="$(target_specific_qdrant_url)"
  if [ -n "$override" ]; then
    echo "$override"
    return 0
  fi

  case "$(target_os "$TARGET")-$(target_arch "$TARGET")" in
    macos-aarch64) echo "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-aarch64-apple-darwin.tar.gz" ;;
    macos-x86_64) echo "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-x86_64-apple-darwin.tar.gz" ;;
    linux-aarch64) echo "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-aarch64-unknown-linux-musl.tar.gz" ;;
    linux-x86_64) echo "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-x86_64-unknown-linux-musl.tar.gz" ;;
    windows-x86_64) echo "https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-x86_64-pc-windows-msvc.zip" ;;
    *) return 1 ;;
  esac
}

stage_path_tool() {
  local tool="$1"
  local dest="$2"
  local src
  src="$(command -v "$tool" || true)"
  if [ -z "$src" ]; then
    echo "Could not find $tool on PATH and no download URL was configured." >&2
    return 1
  fi
  run cp "$src" "$dest"
  run chmod 0755 "$dest"
}

macos_rpaths() {
  local binary="$1"
  otool -l "$binary" 2>/dev/null | awk '
    $1 == "cmd" && $2 == "LC_RPATH" { in_rpath = 1; next }
    in_rpath && $1 == "path" { print $2; in_rpath = 0 }
  '
}

resolve_macos_dependency() {
  local dep="$1"
  local loader="$2"
  local loader_dir
  loader_dir="$(cd "$(dirname "$loader")" && pwd)"

  case "$dep" in
    /usr/lib/*|/System/Library/*)
      return 1
      ;;
    @loader_path/*)
      local path="$loader_dir/${dep#@loader_path/}"
      [ -f "$path" ] && echo "$path"
      ;;
    @executable_path/*)
      local path="$loader_dir/${dep#@executable_path/}"
      [ -f "$path" ] && echo "$path"
      ;;
    @rpath/*)
      local suffix="${dep#@rpath/}"
      local rpath
      while IFS= read -r rpath; do
        rpath="${rpath//@loader_path/$loader_dir}"
        rpath="${rpath//@executable_path/$loader_dir}"
        local path="$rpath/$suffix"
        [ -f "$path" ] && echo "$path" && return 0
      done < <(macos_rpaths "$loader")
      ;;
    /*)
      [ -f "$dep" ] && echo "$dep"
      ;;
  esac
}

is_macho() {
  file "$1" 2>/dev/null | grep -q 'Mach-O'
}

macos_staged_binary_has_unbundled_deps() {
  local binary="$1"
  local dep
  while IFS= read -r dep; do
    case "$dep" in
      /usr/lib/*|/System/Library/*|@loader_path/lib/*)
        ;;
      *)
        return 0
        ;;
    esac
  done < <(otool -L "$binary" 2>/dev/null | awk 'NR > 1 { print $1 }')

  return 1
}

rewrite_macos_dependency() {
  local binary="$1"
  local from="$2"
  local to="$3"

  install_name_tool -change "$from" "$to" "$binary" >/dev/null 2>&1 || true
}

sign_macos_macho() {
  local path="$1"
  if is_macho "$path"; then
    codesign --force --sign - "$path" >/dev/null 2>&1 || true
  fi
}

stage_macos_runtime_libraries() {
  local root_source="$1"
  local root_staged="$2"
  local lib_dir
  lib_dir="$(dirname "$root_staged")/lib"
  local tmp queue seen
  tmp="$(mktemp -d)"
  queue="$tmp/queue"
  seen="$tmp/seen"

  run mkdir -p "$lib_dir"
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ stage non-system macOS runtime libraries for $root_source into $lib_dir"
    rm -rf "$tmp"
    return 0
  fi

  printf '%s\t%s\n' "$root_source" "$root_staged" > "$queue"
  : > "$seen"

  while IFS="$(printf '\t')" read -r current_source current_staged; do
    [ -f "$current_source" ] || continue
    [ -f "$current_staged" ] || continue
    if grep -qxF "$current_staged" "$seen"; then
      continue
    fi
    printf '%s\n' "$current_staged" >> "$seen"

    while IFS= read -r dep; do
      [ -n "$dep" ] || continue
      local resolved
      resolved="$(resolve_macos_dependency "$dep" "$current_source" || true)"
      [ -n "$resolved" ] || continue

      local dep_name dest current_name replacement
      dep_name="$(basename "$resolved")"
      current_name="$(basename "$current_staged")"
      if [ "$dep_name" = "$current_name" ]; then
        continue
      fi

      dest="$lib_dir/$dep_name"
      if [ ! -f "$dest" ]; then
        cp -L "$resolved" "$dest"
        chmod 0755 "$dest"
        xattr -cr "$dest" >/dev/null 2>&1 || true
        echo "Staged macOS runtime library: $dest"
      fi
      if [ "$current_staged" = "$root_staged" ]; then
        replacement="@loader_path/lib/$dep_name"
      else
        replacement="@loader_path/$dep_name"
      fi
      rewrite_macos_dependency "$current_staged" "$dep" "$replacement"
      printf '%s\t%s\n' "$resolved" "$dest" >> "$queue"
    done < <(otool -L "$current_source" 2>/dev/null | awk 'NR > 1 { print $1 }')

    if [ "$current_staged" != "$root_staged" ]; then
      install_name_tool -id "@loader_path/$(basename "$current_staged")" "$current_staged" >/dev/null 2>&1 || true
    fi
  done < "$queue"

  while IFS= read -r lib; do
    sign_macos_macho "$lib"
  done < <(find "$lib_dir" -type f -print 2>/dev/null)
  sign_macos_macho "$root_staged"

  rm -rf "$tmp"
}

stage_ffmpeg() {
  if FFMPEG_URL="$(ffmpeg_url)"; then
    stage_from_archive "$FFMPEG_URL" "$FFMPEG_EXE" "$FFMPEG_OUT"
    if [ "$(target_os "$TARGET")" = "macos" ]; then
      stage_macos_runtime_libraries "$FFMPEG_OUT" "$FFMPEG_OUT"
    fi
  else
    if [ "${CERUL_RELEASE_BUILD:-0}" = "1" ]; then
      # Release builds must not ship whatever ffmpeg happens to be on the
      # build machine's PATH: it's unreproducible and Homebrew builds are
      # typically GPL (x264) — a licence conflict in an Apache-2.0 installer.
      echo "Release builds require CERUL_FFMPEG_URL (PATH fallback disabled)." >&2
      return 1
    fi
    local src
    src="$(command -v "$FFMPEG_EXE" || true)"
    if [ -z "$src" ]; then
      echo "Could not find $FFMPEG_EXE on PATH and no download URL was configured." >&2
      return 1
    fi
    run cp "$src" "$FFMPEG_OUT"
    run chmod 0755 "$FFMPEG_OUT"
    if [ "$(target_os "$TARGET")" = "macos" ]; then
      rm -rf "$(dirname "$FFMPEG_OUT")/lib"
      stage_macos_runtime_libraries "$src" "$FFMPEG_OUT"
    fi
  fi
}

run_probe() {
  local path="$1"
  shift
  local timeout_sec="${CERUL_BINARY_PROBE_TIMEOUT_SEC:-60}"

  if command -v timeout >/dev/null 2>&1; then
    timeout "$timeout_sec" "$path" "$@" >/dev/null 2>&1
    return $?
  fi

  "$path" "$@" >/dev/null 2>&1 &
  local pid="$!"
  local i
  for ((i = 1; i <= timeout_sec; i++)); do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      wait "$pid"
      return $?
    fi
    sleep 1
  done

  kill -9 "$pid" >/dev/null 2>&1 || true
  return 124
}

verify_staged_binary() {
  local name="$1"
  local path="$2"
  shift 2
  if [ "$DRY_RUN" -eq 1 ]; then
    return 0
  fi
  if [ ! -x "$path" ]; then
    echo "Staged $name is missing or not executable: $path" >&2
    return 1
  fi
  if [ "$TARGET" != "$HOST_TARGET" ]; then
    echo "Skipping runtime probe for cross-target $name binary: $path"
    return 0
  fi
  if ! run_probe "$path" "$@"; then
    echo "Staged $name is not runnable: $path" >&2
    echo "Use a standalone build or an archive that includes its required runtime libraries." >&2
    return 1
  fi
}

needs_stage_binary() {
  local name="$1"
  local path="$2"
  shift 2

  if [ "$FORCE" -eq 1 ] || [ ! -x "$path" ]; then
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ] || [ "$TARGET" != "$HOST_TARGET" ]; then
    return 1
  fi
  if [ "$name" = "ffmpeg" ] &&
    [ "$(target_os "$TARGET")" = "macos" ] &&
    macos_staged_binary_has_unbundled_deps "$path"; then
    echo "Existing staged ffmpeg has unbundled macOS dynamic library paths; staging a fresh copy: $path" >&2
    return 0
  fi
  if run_probe "$path" "$@"; then
    return 1
  fi

  echo "Existing staged $name is not runnable; staging a fresh copy: $path" >&2
  return 0
}

OUT_DIR="$ROOT/third-party/$TARGET"
run mkdir -p "$OUT_DIR"

FFMPEG_EXE="ffmpeg$(exe_suffix)"
YTDLP_EXE="yt-dlp$(exe_suffix)"
QDRANT_EXE="qdrant$(exe_suffix)"
FFMPEG_OUT="$OUT_DIR/$FFMPEG_EXE"
YTDLP_OUT="$OUT_DIR/$YTDLP_EXE"
QDRANT_OUT="$OUT_DIR/$QDRANT_EXE"

if needs_stage_binary "ffmpeg" "$FFMPEG_OUT" -version; then
  stage_ffmpeg
fi

if needs_stage_binary "yt-dlp" "$YTDLP_OUT" --version; then
  if URL="$(ytdlp_url)"; then
    if ! stage_from_archive "$URL" "$YTDLP_EXE" "$YTDLP_OUT"; then
      stage_path_tool "$YTDLP_EXE" "$YTDLP_OUT"
    fi
  else
    stage_path_tool "$YTDLP_EXE" "$YTDLP_OUT"
  fi
fi

if needs_stage_binary "qdrant" "$QDRANT_OUT" --version; then
  if URL="$(qdrant_url)"; then
    if ! stage_from_archive "$URL" "$QDRANT_EXE" "$QDRANT_OUT"; then
      stage_path_tool "$QDRANT_EXE" "$QDRANT_OUT"
    fi
  else
    stage_path_tool "$QDRANT_EXE" "$QDRANT_OUT"
  fi
fi

verify_staged_binary "ffmpeg" "$FFMPEG_OUT" -version
verify_staged_binary "yt-dlp" "$YTDLP_OUT" --version
verify_staged_binary "qdrant" "$QDRANT_OUT" --version

if [ "$DRY_RUN" -eq 1 ]; then
  echo "Would stage bundled binaries for $TARGET:"
else
  echo "Staged bundled binaries for $TARGET:"
fi
echo "  $FFMPEG_OUT"
echo "  $YTDLP_OUT"
echo "  $QDRANT_OUT"
