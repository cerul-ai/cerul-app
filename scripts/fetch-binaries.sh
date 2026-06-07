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
CERUL_FFMPEG_URL_<TARGET> is set. The URL may point to a binary, .zip, .tar.gz,
or .tar.xz archive containing an ffmpeg executable.

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

target_specific_qdrant_url() {
  local env_name="CERUL_QDRANT_DOWNLOAD_URL_$(sanitize_env_target "$TARGET")"
  printf '%s' "${!env_name:-${CERUL_QDRANT_DOWNLOAD_URL:-}}"
}

ytdlp_url() {
  case "$(target_os "$TARGET")-$(target_arch "$TARGET")" in
    macos-*) echo "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos" ;;
    linux-aarch64) echo "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64" ;;
    linux-x86_64) echo "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux" ;;
    windows-*) echo "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" ;;
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
    macos-aarch64) echo "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-aarch64-apple-darwin.tar.gz" ;;
    macos-x86_64) echo "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-x86_64-apple-darwin.tar.gz" ;;
    linux-aarch64) echo "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-aarch64-unknown-linux-musl.tar.gz" ;;
    linux-x86_64) echo "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-x86_64-unknown-linux-musl.tar.gz" ;;
    windows-x86_64) echo "https://github.com/qdrant/qdrant/releases/latest/download/qdrant-x86_64-pc-windows-msvc.zip" ;;
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
  if ! "$path" "$@" >/dev/null 2>&1; then
    echo "Staged $name is not runnable: $path" >&2
    echo "Use a standalone build or an archive that includes its required runtime libraries." >&2
    return 1
  fi
}

OUT_DIR="$ROOT/third-party/$TARGET"
run mkdir -p "$OUT_DIR"

FFMPEG_EXE="ffmpeg$(exe_suffix)"
YTDLP_EXE="yt-dlp$(exe_suffix)"
QDRANT_EXE="qdrant$(exe_suffix)"
FFMPEG_OUT="$OUT_DIR/$FFMPEG_EXE"
YTDLP_OUT="$OUT_DIR/$YTDLP_EXE"
QDRANT_OUT="$OUT_DIR/$QDRANT_EXE"

if [ "$FORCE" -eq 1 ] || [ ! -x "$FFMPEG_OUT" ]; then
  FFMPEG_URL="$(target_specific_ffmpeg_url)"
  if [ -n "$FFMPEG_URL" ]; then
    stage_from_archive "$FFMPEG_URL" "$FFMPEG_EXE" "$FFMPEG_OUT"
  else
    stage_path_tool "$FFMPEG_EXE" "$FFMPEG_OUT"
  fi
fi

if [ "$FORCE" -eq 1 ] || [ ! -x "$YTDLP_OUT" ]; then
  if URL="$(ytdlp_url)"; then
    if ! stage_from_archive "$URL" "$YTDLP_EXE" "$YTDLP_OUT"; then
      stage_path_tool "$YTDLP_EXE" "$YTDLP_OUT"
    fi
  else
    stage_path_tool "$YTDLP_EXE" "$YTDLP_OUT"
  fi
fi

if [ "$FORCE" -eq 1 ] || [ ! -x "$QDRANT_OUT" ]; then
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
