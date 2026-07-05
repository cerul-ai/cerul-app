#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cd "$ROOT"

cargo build -q -p cerul-cli
target_dir="${CARGO_TARGET_DIR:-$ROOT/target}"
cerul_cli="$target_dir/debug/cerul-cli"
if [ ! -x "$cerul_cli" ] && [ -x "$target_dir/debug/cerul-cli.exe" ]; then
  cerul_cli="$target_dir/debug/cerul-cli.exe"
fi
export DYLD_LIBRARY_PATH="$target_dir/debug${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
export LD_LIBRARY_PATH="$target_dir/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

AUDIO_DIR="$TMP_DIR/audio"
IMAGE_DIR="$TMP_DIR/images"
DATA_DIR="$TMP_DIR/data"
mkdir -p "$AUDIO_DIR" "$IMAGE_DIR"

printf 'audio-one' >"$AUDIO_DIR/episode-1.mp3"
printf 'audio-two' >"$AUDIO_DIR/episode-2.WAV"
printf 'not-audio' >"$AUDIO_DIR/notes.txt"

printf 'image-one' >"$IMAGE_DIR/photo-1.jpg"
printf 'image-two' >"$IMAGE_DIR/photo-2.PNG"
printf 'not-image' >"$IMAGE_DIR/notes.txt"

audio_list="$(
  "$cerul_cli" \
    list-source folder_audio \
    --path "$AUDIO_DIR"
)"
audio_count="$(printf '%s\n' "$audio_list" | grep -c $'\tepisode-')"
if [ "$audio_count" -ne 2 ]; then
  echo "Expected 2 discovered audio files, got $audio_count." >&2
  printf '%s\n' "$audio_list" >&2
  exit 1
fi

image_list="$(
  "$cerul_cli" \
    list-source folder_image \
    --path "$IMAGE_DIR"
)"
image_count="$(printf '%s\n' "$image_list" | grep -c $'\tphoto-')"
if [ "$image_count" -ne 2 ]; then
  echo "Expected 2 discovered image files, got $image_count." >&2
  printf '%s\n' "$image_list" >&2
  exit 1
fi

audio_add="$(
  "$cerul_cli" \
    --data-dir "$DATA_DIR" \
    add-source folder_audio \
    --path "$AUDIO_DIR"
)"
if ! printf '%s\n' "$audio_add" | grep -q $'^source\t.*\tfolder_audio\tactive$'; then
  echo "Audio source was not persisted as active." >&2
  printf '%s\n' "$audio_add" >&2
  exit 1
fi
if ! printf '%s\n' "$audio_add" | grep -q $'^jobs\t2$'; then
  echo "Expected 2 queued audio index jobs." >&2
  printf '%s\n' "$audio_add" >&2
  exit 1
fi

image_add="$(
  "$cerul_cli" \
    --data-dir "$DATA_DIR" \
    add-source folder_image \
    --path "$IMAGE_DIR"
)"
if ! printf '%s\n' "$image_add" | grep -q $'^source\t.*\tfolder_image\tactive$'; then
  echo "Image source was not persisted as active." >&2
  printf '%s\n' "$image_add" >&2
  exit 1
fi
if ! printf '%s\n' "$image_add" | grep -q $'^jobs\t2$'; then
  echo "Expected 2 queued image index jobs." >&2
  printf '%s\n' "$image_add" >&2
  exit 1
fi

cargo test -q -p cerul-api chunk_frame_endpoint_serves_source_image_content_types

echo "audio_image_sources_smoke audio_discovered=2 audio_queued=2 image_discovered=2 image_queued=2 frame_mime=extension_aware"
