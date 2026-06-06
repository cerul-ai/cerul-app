#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRY_RUN=0
CHECK_PREREQS=0
RUN_WHISPER=0
RUN_QWEN3=0
RUN_FAST=0
EXPLICIT=0
MODELS_CACHE="${CERUL_MODEL_SMOKE_CACHE:-}"
MODEL_RETRIES="${CERUL_MODEL_SMOKE_RETRIES:-2}"

usage() {
  cat <<'EOF'
Usage: scripts/smoke-real-models.sh [--all] [--whisper] [--qwen3] [--fast] [--models-cache <path>] [--retries <n>] [--check-prereqs] [--dry-run]

Runs the release-gated model smokes that are intentionally excluded from normal
CI because they require large downloads or local media fixtures.

Whisper requires:
  CERUL_WHISPER_MODEL_PATH       path to a ggml Whisper model
  CERUL_WHISPER_SAMPLE_WAV       path to a 16 kHz WAV sample with speech

Qwen3 and fastembed fallback smokes download models through Hugging Face on
first run. Use --models-cache or CERUL_MODEL_SMOKE_CACHE to pin downloads to a
release-evidence directory. The script exports HF_HOME and FASTEMBED_CACHE_DIR
to that path before model tests run. Model tests retry twice by default because
Hugging Face downloads can fail transiently.

Use --check-prereqs to print machine-readable prerequisite status without
running cargo tests, creating cache directories, or downloading models.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --all)
      RUN_WHISPER=1
      RUN_QWEN3=1
      RUN_FAST=1
      EXPLICIT=1
      shift
      ;;
    --whisper)
      RUN_WHISPER=1
      EXPLICIT=1
      shift
      ;;
    --qwen3)
      RUN_QWEN3=1
      EXPLICIT=1
      shift
      ;;
    --fast)
      RUN_FAST=1
      EXPLICIT=1
      shift
      ;;
    --models-cache)
      MODELS_CACHE="${2:?missing models cache path}"
      shift 2
      ;;
    --retries)
      MODEL_RETRIES="${2:?missing retry count}"
      shift 2
      ;;
    --check-prereqs)
      CHECK_PREREQS=1
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

if [ "$EXPLICIT" -eq 0 ]; then
  RUN_WHISPER=1
  RUN_QWEN3=1
  RUN_FAST=1
fi

case "$MODEL_RETRIES" in
  ''|*[!0-9]*)
    echo "--retries must be a positive integer." >&2
    exit 2
    ;;
esac

if [ "$MODEL_RETRIES" -lt 1 ]; then
  echo "--retries must be greater than zero." >&2
  exit 2
fi

cd "$ROOT"

QWEN3_VL_REPO_ID="Qwen/Qwen3-VL-Embedding-2B"
FAST_BACKEND_REPO_ID="$QWEN3_VL_REPO_ID"

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

run_model_test() {
  local label="$1"
  shift

  if [ "$DRY_RUN" -eq 1 ]; then
    run "$@"
    return
  fi

  local attempt=1
  while [ "$attempt" -le "$MODEL_RETRIES" ]; do
    if "$@"; then
      return
    fi

    if [ "$attempt" -eq "$MODEL_RETRIES" ]; then
      echo "$label failed after $MODEL_RETRIES attempt(s)." >&2
      return 1
    fi

    echo "$label failed on attempt $attempt/$MODEL_RETRIES; retrying." >&2
    attempt=$((attempt + 1))
    sleep $((attempt * 5))
  done
}

field() {
  local key="$1"
  local value="$2"
  printf '%s=%q' "$key" "$value"
}

emit_prereq() {
  local gate="$1"
  local name="$2"
  local status="$3"
  shift 3

  printf 'real_model_prereq gate=%s name=%s status=%s' "$gate" "$name" "$status"
  while [ "$#" -gt 0 ]; do
    printf ' %s' "$1"
    shift
  done
  printf '\n'
}

prereq_failures=0

mark_prereq_failed() {
  prereq_failures=$((prereq_failures + 1))
}

nearest_existing_ancestor() {
  local path="$1"
  local parent
  parent="$(dirname "$path")"

  while [ "$parent" != "/" ] && [ ! -e "$parent" ]; do
    parent="$(dirname "$parent")"
  done

  printf '%s' "$parent"
}

check_command_prereq() {
  local name="$1"

  if command -v "$name" >/dev/null 2>&1; then
    emit_prereq local "$name" ok "$(field path "$(command -v "$name")")"
    return
  fi

  emit_prereq local "$name" missing "$(field detail "required to run Rust model smoke tests")"
  mark_prereq_failed
}

check_file_env_prereq() {
  local gate="$1"
  local name="$2"
  local description="$3"
  local value="${!name:-}"

  if [ -z "$value" ]; then
    emit_prereq "$gate" "$name" missing "$(field detail "$description")"
    mark_prereq_failed
    return
  fi

  if [ ! -f "$value" ]; then
    emit_prereq "$gate" "$name" missing "$(field path "$value")" "$(field detail "file not found")"
    mark_prereq_failed
    return
  fi

  emit_prereq "$gate" "$name" ok "$(field path "$value")" "$(field detail "$description")"
}

check_models_cache_prereq() {
  if [ -z "$MODELS_CACHE" ]; then
    emit_prereq models models_cache missing "$(field detail "pass --models-cache or set CERUL_MODEL_SMOKE_CACHE for release evidence")"
    mark_prereq_failed
    return
  fi

  if [ -e "$MODELS_CACHE" ] && [ ! -d "$MODELS_CACHE" ]; then
    emit_prereq models models_cache invalid "$(field path "$MODELS_CACHE")" "$(field detail "path exists but is not a directory")"
    mark_prereq_failed
    return
  fi

  if [ -d "$MODELS_CACHE" ]; then
    if [ -w "$MODELS_CACHE" ]; then
      emit_prereq models models_cache ok "$(field path "$MODELS_CACHE")"
      return
    fi

    emit_prereq models models_cache invalid "$(field path "$MODELS_CACHE")" "$(field detail "directory is not writable")"
    mark_prereq_failed
    return
  fi

  local ancestor
  ancestor="$(nearest_existing_ancestor "$MODELS_CACHE")"
  if [ -d "$ancestor" ] && [ -w "$ancestor" ]; then
    emit_prereq models models_cache will_create "$(field path "$MODELS_CACHE")" "$(field ancestor "$ancestor")"
    return
  fi

  emit_prereq models models_cache invalid "$(field path "$MODELS_CACHE")" "$(field ancestor "$ancestor")" "$(field detail "nearest existing ancestor is not writable")"
  mark_prereq_failed
}

run_prereq_check() {
  local selected=()
  prereq_failures=0

  check_command_prereq cargo

  if [ "$RUN_WHISPER" -eq 1 ]; then
    selected+=(whisper)
    check_file_env_prereq whisper CERUL_WHISPER_MODEL_PATH "ggml Whisper model"
    check_file_env_prereq whisper CERUL_WHISPER_SAMPLE_WAV "16 kHz speech WAV"
  fi

  if [ "$RUN_QWEN3" -eq 1 ] || [ "$RUN_FAST" -eq 1 ]; then
    check_models_cache_prereq
  fi

  if [ "$RUN_QWEN3" -eq 1 ]; then
    selected+=(qwen3)
    emit_prereq qwen3 huggingface_download required "$(field repo "$QWEN3_VL_REPO_ID")" "estimate_mib=4096"
  fi

  if [ "$RUN_FAST" -eq 1 ]; then
    selected+=(fast)
    emit_prereq fast huggingface_download required "$(field repo "$FAST_BACKEND_REPO_ID")"
  fi

  printf 'real_model_prereq_check'
  if [ "$prereq_failures" -eq 0 ]; then
    printf ' status=passed'
  else
    printf ' status=failed failures=%s' "$prereq_failures"
  fi
  for name in "${selected[@]}"; do
    printf ' %s=selected' "$name"
  done
  if [ -n "$MODELS_CACHE" ]; then
    printf ' models_cache=%q' "$MODELS_CACHE"
  fi
  printf '\n'

  if [ "$prereq_failures" -gt 0 ]; then
    return 2
  fi
}

if [ "$CHECK_PREREQS" -eq 1 ]; then
  run_prereq_check
  exit $?
fi

if [ -n "$MODELS_CACHE" ]; then
  export HF_HOME="$MODELS_CACHE/huggingface"
  export FASTEMBED_CACHE_DIR="$MODELS_CACHE/fastembed"
  run mkdir -p "$HF_HOME" "$FASTEMBED_CACHE_DIR"
fi

require_file_env() {
  local name="$1"
  local description="$2"
  local value="${!name:-}"

  if [ -z "$value" ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
      echo "# requires $name ($description)"
      return
    fi
    echo "$name is required for the Whisper model smoke ($description)." >&2
    exit 2
  fi

  if [ ! -f "$value" ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
      echo "# $name points to a missing file: $value"
      return
    fi
    echo "$name does not point to a file: $value" >&2
    exit 2
  fi
}

ran=()

if [ "$RUN_WHISPER" -eq 1 ]; then
  require_file_env CERUL_WHISPER_MODEL_PATH "ggml Whisper model"
  require_file_env CERUL_WHISPER_SAMPLE_WAV "16 kHz speech WAV"
  run_model_test whisper cargo test -p cerul-pipeline whisper_transcribe_sample -- --ignored --nocapture
  ran+=(whisper)
fi

if [ "$RUN_QWEN3" -eq 1 ]; then
  run_model_test qwen3 cargo test -p cerul-embed --release qwen3_smoke -- --ignored --nocapture
  ran+=(qwen3)
fi

if [ "$RUN_FAST" -eq 1 ]; then
  run_model_test fast cargo test -p cerul-embed --release fast_backend_smoke -- --ignored --nocapture
  ran+=(fast)
fi

if [ -n "$MODELS_CACHE" ]; then
  run scripts/smoke-release-footprint.sh --models-cache "$MODELS_CACHE" --models-cache-only --max-installer-mib 0
fi

printf 'real_model_smoke'
status_label="passed"
if [ "$DRY_RUN" -eq 1 ]; then
  status_label="planned"
fi
for name in "${ran[@]}"; do
  printf ' %s=%s' "$name" "$status_label"
done
if [ -n "$MODELS_CACHE" ]; then
  printf ' models_cache=%s' "$MODELS_CACHE"
fi
printf ' retries=%s' "$MODEL_RETRIES"
printf '\n'
