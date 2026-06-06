#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cd "$ROOT"

cargo test -q -p cerul-api chunk_frame_endpoint_serves_source_image_content_types

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
  cargo run -q -p cerul-cli -- \
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
  cargo run -q -p cerul-cli -- \
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
  cargo run -q -p cerul-cli -- \
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
  cargo run -q -p cerul-cli -- \
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

echo "audio_image_sources_smoke audio_discovered=2 audio_queued=2 image_discovered=2 image_queued=2 frame_mime=extension_aware"
