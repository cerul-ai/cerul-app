#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

source scripts/load-env.sh
CARGO_JOBS="${CARGO_BUILD_JOBS:-1}"
CARGO_RETRIES="${CERUL_REBUILD_CARGO_RETRIES:-16}"
RETRY_SLEEP="${CERUL_REBUILD_RETRY_SLEEP:-8}"
STEP_RETRIES="${CERUL_REBUILD_STEP_RETRIES:-4}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG:-0}"

usage() {
  cat <<'EOF'
Usage: ./rebuild.sh

Fully rebuilds and validates the local desktop app, then starts Electron.

This intentionally clears build outputs and Cargo artifacts. Use ./run.sh for
fast incremental local startup.

Environment:
  CARGO_BUILD_JOBS              Rust build parallelism, default 1
  CERUL_REBUILD_CARGO_RETRIES   Cargo SIGKILL retry attempts, default 16
  CERUL_REBUILD_RETRY_SLEEP     Seconds between SIGKILL retries, default 8
  CERUL_REBUILD_STEP_RETRIES    Non-cargo SIGKILL retry attempts, default 4
  CARGO_INCREMENTAL             Rust incremental compilation, default 0 here
  CARGO_PROFILE_DEV_DEBUG       Dev debug info level, default 0 here
  CARGO_PROFILE_TEST_DEBUG      Test/check debug info level, default 0 here
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
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

run_step() {
  echo "+ $*"
  "$@"
}

run_retry_on_sigkill() {
  echo "+ $*"
  local attempt=1
  while true; do
    set +e
    "$@"
    local status=$?
    set -e
    if [ "$status" -eq 0 ]; then
      return 0
    fi
    if [ "$status" -eq 137 ] && [ "$attempt" -lt "$STEP_RETRIES" ]; then
      echo "Command was killed by SIGKILL; retrying attempt $((attempt + 1))/$STEP_RETRIES after a short pause." >&2
      attempt=$((attempt + 1))
      sleep "$RETRY_SLEEP"
      echo "+ retry $*"
      continue
    fi
    return "$status"
  done
}

cargo_recover_after_sigkill() {
  local log_file="$1"
  local packages
  packages="$(sed -nE 's/.*failed to run custom build command for `([^` ]+) v[0-9].*/\1/p' "$log_file" | sort -u)"
  if [ -z "$packages" ]; then
    echo "Cleaning Cargo target artifacts before retry; SIGKILL can leave fingerprints or proc-macro artifacts half-written." >&2
    cargo clean
    return
  fi

  local package
  for package in $packages; do
    echo "Cleaning Cargo artifacts for killed package: $package" >&2
    cargo clean -p "$package" || {
      echo "Targeted clean failed; cleaning all Cargo target artifacts before retry." >&2
      cargo clean
      return
    }
  done
}

is_cargo_target_corruption() {
  local log_file="$1"
  grep -Eq "can't find crate for|found invalid metadata|failed to read .*\\.rmeta|metadata version mismatch" "$log_file"
}

run_retry_on_cargo_sigkill() {
  echo "+ $*"
  local attempt=1
  local log_file
  while true; do
    log_file="$(mktemp)"
    set +e
    "$@" 2>&1 | tee "$log_file"
    local status="${PIPESTATUS[0]}"
    set -e
    if [ "$status" -eq 0 ]; then
      rm -f "$log_file"
      return 0
    fi
    if [ "$attempt" -lt "$CARGO_RETRIES" ]; then
      if grep -Eq "SIGKILL|signal: 9|Killed: 9" "$log_file"; then
        echo "Cargo was killed by SIGKILL; retrying attempt $((attempt + 1))/$CARGO_RETRIES after a short pause." >&2
        cargo_recover_after_sigkill "$log_file"
        rm -f "$log_file"
        attempt=$((attempt + 1))
        sleep "$RETRY_SLEEP"
        echo "+ retry $*"
        continue
      fi
      if is_cargo_target_corruption "$log_file"; then
        echo "Cargo target artifacts look corrupted; cleaning all Cargo target artifacts before retry attempt $((attempt + 1))/$CARGO_RETRIES." >&2
        rm -f "$log_file"
        cargo clean
        attempt=$((attempt + 1))
        sleep "$RETRY_SLEEP"
        echo "+ retry $*"
        continue
      fi
    fi
    rm -f "$log_file"
    return "$status"
  done
}

rm -rf apps/desktop/dist .cache .turbo .cerul
run_step cargo clean
run_step pnpm install
run_retry_on_sigkill pnpm --filter @cerul/desktop build
run_retry_on_cargo_sigkill cargo build -p cerul-api -j "$CARGO_JOBS"
run_retry_on_cargo_sigkill cargo check --workspace -j "$CARGO_JOBS"
run_step pnpm --filter @cerul/electron-shell build
run_step pnpm --filter @cerul/electron-shell exec electron --version
run_step pnpm --filter @cerul/electron-shell start
