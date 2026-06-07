#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRY_RUN=0
SKIP_INSTALLERS=0
SKIP_FETCH=0
DEBUG_INSTALLERS=0
RUN_REAL_MODELS=0
RUN_LIVE_YOUTUBE=0
RUN_LIVE_YOUTUBE_FETCH_FIRST=0
RUN_LIVE_YOUTUBE_INDEX_FIRST=0
RUN_LIVE_YOUTUBE_INDEX_COUNT=1
RUN_REAL_MODEL_PREREQS=0
RUN_SIGNED_ARTIFACTS=0
RUN_INSTALLED_HOTKEY=0
RUN_INSTALLED_HOTKEY_MANUAL=0
MODELS_CACHE="${CERUL_MODEL_SMOKE_CACHE:-}"
MODEL_RETRIES="${CERUL_MODEL_SMOKE_RETRIES:-2}"
REPORT_PATH="$ROOT/.tmp/smoke-release-checklist.md"
DEFAULT_APP_VERSION="0.0.1-alpha.1"

if command -v node >/dev/null 2>&1 && [ -f "$ROOT/apps/electron-shell/package.json" ]; then
  DEFAULT_APP_VERSION="$(cd "$ROOT" && node -p "require('./apps/electron-shell/package.json').version")"
fi

usage() {
  cat <<'EOF'
Usage: scripts/smoke-release.sh [--dry-run] [--debug-installers] [--skip-installers] [--skip-fetch] [--real-model-prereqs] [--real-models] [--models-cache <path>] [--model-retries <n>] [--live-youtube] [--live-youtube-fetch-first] [--live-youtube-index-first] [--live-youtube-index-five] [--signed-artifacts] [--installed-hotkey] [--installed-hotkey-manual] [--report <path>]

Runs the automated pre-ship smoke suite and writes a manual checklist for the
product smokes. Installer smokes
build release artifacts by default; use --debug-installers only for local loops.
Use --real-models only when the machine is allowed to download large model
artifacts and CERUL_WHISPER_MODEL_PATH / CERUL_WHISPER_SAMPLE_WAV are set.
Use --models-cache to pin Hugging Face / fastembed downloads to a measurable
release-evidence directory when --real-models is enabled.
Use --model-retries to control transient Hugging Face download retries.
Use --real-model-prereqs to fail fast on missing real-model fixtures/cache
without running model tests or downloading model artifacts.
Use --live-youtube to run the bounded real yt-dlp discovery/queueing smoke
against https://www.youtube.com/@karpathy.
Use --live-youtube-index-first to fetch and index the first live YouTube video
with smoke model adapters, then verify the indexed chunk is searchable.
Use --live-youtube-index-five to fetch short clips from the first five live
YouTube videos, index each with smoke model adapters, and verify each one is
searchable.
Use --signed-artifacts only for public macOS release candidates built with
Developer ID signing and notarization enabled.
Use --installed-hotkey only on an interactive macOS session with Accessibility
permission granted to the terminal/Codex runner.
Use --installed-hotkey-manual when synthetic keypresses do not reach the macOS
global shortcut handler; it waits for a physical hotkey press and verifies the
same installed-app overlay behavior.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --skip-installers)
      SKIP_INSTALLERS=1
      shift
      ;;
    --debug-installers)
      DEBUG_INSTALLERS=1
      shift
      ;;
    --skip-fetch)
      SKIP_FETCH=1
      shift
      ;;
    --real-models)
      RUN_REAL_MODELS=1
      shift
      ;;
    --real-model-prereqs)
      RUN_REAL_MODEL_PREREQS=1
      shift
      ;;
    --models-cache)
      MODELS_CACHE="${2:?missing models cache path}"
      shift 2
      ;;
    --model-retries)
      MODEL_RETRIES="${2:?missing retry count}"
      shift 2
      ;;
    --live-youtube)
      RUN_LIVE_YOUTUBE=1
      shift
      ;;
    --live-youtube-fetch-first)
      RUN_LIVE_YOUTUBE=1
      RUN_LIVE_YOUTUBE_FETCH_FIRST=1
      shift
      ;;
    --live-youtube-index-first)
      RUN_LIVE_YOUTUBE=1
      RUN_LIVE_YOUTUBE_FETCH_FIRST=1
      RUN_LIVE_YOUTUBE_INDEX_FIRST=1
      RUN_LIVE_YOUTUBE_INDEX_COUNT=1
      shift
      ;;
    --live-youtube-index-five)
      RUN_LIVE_YOUTUBE=1
      RUN_LIVE_YOUTUBE_FETCH_FIRST=1
      RUN_LIVE_YOUTUBE_INDEX_FIRST=1
      RUN_LIVE_YOUTUBE_INDEX_COUNT=5
      shift
      ;;
    --signed-artifacts)
      RUN_SIGNED_ARTIFACTS=1
      shift
      ;;
    --installed-hotkey)
      RUN_INSTALLED_HOTKEY=1
      shift
      ;;
    --installed-hotkey-manual)
      RUN_INSTALLED_HOTKEY=1
      RUN_INSTALLED_HOTKEY_MANUAL=1
      shift
      ;;
    --report)
      REPORT_PATH="${2:?missing report path}"
      shift 2
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

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

if [ "$RUN_REAL_MODEL_PREREQS" -eq 1 ]; then
  real_model_prereq_args=(--all --check-prereqs)
  if [ -n "$MODELS_CACHE" ]; then
    real_model_prereq_args+=(--models-cache "$MODELS_CACHE")
  fi
  run scripts/smoke-real-models.sh "${real_model_prereq_args[@]}"
fi

run scripts/smoke.sh
run scripts/smoke-folder-happy-path.sh
run scripts/smoke-youtube-source.sh --index-first --index-count 5
if [ "$RUN_LIVE_YOUTUBE" -eq 1 ]; then
  live_youtube_args=(--live)
  if [ "$RUN_LIVE_YOUTUBE_FETCH_FIRST" -eq 1 ]; then
    live_youtube_args+=(--fetch-first)
  fi
  if [ "$RUN_LIVE_YOUTUBE_INDEX_FIRST" -eq 1 ]; then
    live_youtube_args+=(--index-first --index-count "$RUN_LIVE_YOUTUBE_INDEX_COUNT" --max "$RUN_LIVE_YOUTUBE_INDEX_COUNT" --clip-duration-sec 12)
  fi
  run scripts/smoke-youtube-source.sh "${live_youtube_args[@]}"
fi
run scripts/smoke-hotkey-overlay.sh
run cargo test -p cerul-pipeline process_video_item_writes_sqlite_and_qdrant
run cargo test -p cerul-pipeline audio_image_smoke
run scripts/smoke-audio-image-sources.sh
run cargo test -p cerul-search
run scripts/smoke-search-latency.sh
run scripts/smoke-restart-resilience.sh
if [ "$RUN_REAL_MODELS" -eq 1 ]; then
  real_model_args=(--all --retries "$MODEL_RETRIES")
  if [ -n "$MODELS_CACHE" ]; then
    real_model_args+=(--models-cache "$MODELS_CACHE")
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    real_model_args+=(--dry-run)
  fi
  run scripts/smoke-real-models.sh "${real_model_args[@]}"
fi
run cargo test -p cerul-api
run pnpm --filter @cerul/electron-shell package

if [ "$SKIP_INSTALLERS" -eq 0 ]; then
  installer_args=()
  installer_profile="release"
  if [ "$DEBUG_INSTALLERS" -eq 1 ]; then
    installer_args+=(--debug)
    installer_profile="debug"
  fi
  if [ "$SKIP_FETCH" -eq 1 ]; then
    installer_args+=(--skip-fetch)
  fi
  if [ "$(uname -s)" = "Darwin" ] && [ "$RUN_SIGNED_ARTIFACTS" -eq 1 ]; then
    installer_args+=(--require-signing)
  fi
  run scripts/build-installers.sh "${installer_args[@]}"
  artifact_args=(--profile "$installer_profile")
  if [ "$DEBUG_INSTALLERS" -eq 1 ]; then
    artifact_args+=(--dir-only)
  fi
  run scripts/smoke-release-artifacts.sh "${artifact_args[@]}"
  footprint_args=(--profile "$installer_profile")
  if [ "$DEBUG_INSTALLERS" -eq 1 ]; then
    footprint_args+=(--models-cache-only)
  fi
  if [ -n "$MODELS_CACHE" ]; then
    footprint_args+=(--models-cache "$MODELS_CACHE")
  fi
  run scripts/smoke-release-footprint.sh "${footprint_args[@]}"
  if [ "$(uname -s)" = "Darwin" ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
      dmg_path="target/electron/Cerul-$DEFAULT_APP_VERSION-arm64.dmg"
      run scripts/smoke-installed-app.sh "$dmg_path"
      run scripts/smoke-daemon-autostart-macos.sh --dmg "$dmg_path"
      if [ "$RUN_INSTALLED_HOTKEY" -eq 1 ]; then
        installed_hotkey_args=(--dmg "$dmg_path")
        if [ "$RUN_INSTALLED_HOTKEY_MANUAL" -eq 1 ]; then
          installed_hotkey_args+=(--manual)
        fi
        run scripts/smoke-installed-hotkey-macos.sh "${installed_hotkey_args[@]}"
      fi
      if [ "$RUN_SIGNED_ARTIFACTS" -eq 1 ]; then
        run scripts/smoke-macos-signing.sh --dmg "$dmg_path"
      fi
    else
      dmg_path="$(find "$ROOT/target/electron" -maxdepth 2 -name "*.dmg" -type f -print 2>/dev/null | sort | tail -1)"
      if [ -z "$dmg_path" ]; then
        echo "No Electron DMG found under target/electron." >&2
        exit 1
      fi
      run scripts/smoke-installed-app.sh "$dmg_path"
      run scripts/smoke-daemon-autostart-macos.sh --dmg "$dmg_path"
      if [ "$RUN_INSTALLED_HOTKEY" -eq 1 ]; then
        installed_hotkey_args=(--dmg "$dmg_path")
        if [ "$RUN_INSTALLED_HOTKEY_MANUAL" -eq 1 ]; then
          installed_hotkey_args+=(--manual)
        fi
        run scripts/smoke-installed-hotkey-macos.sh "${installed_hotkey_args[@]}"
      fi
      if [ "$RUN_SIGNED_ARTIFACTS" -eq 1 ]; then
        run scripts/smoke-macos-signing.sh --dmg "$dmg_path"
      fi
    fi
  elif [ "$(uname -s)" = "Linux" ]; then
    run scripts/smoke-daemon-autostart-linux.sh
    run scripts/smoke-installed-runtime-linux.sh
  elif [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* ]]; then
    run scripts/smoke-daemon-autostart-windows.sh
    run scripts/smoke-installed-runtime-windows.sh
  fi
fi

run mkdir -p "$(dirname "$REPORT_PATH")"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ write manual checklist $REPORT_PATH"
else
  cat >"$REPORT_PATH" <<'EOF'
# Cerul Release Smoke Checklist

Record the exact build, machine, dataset, and result for each smoke before a public release.

Automated release smoke builds and verifies release installers by default. Use
`scripts/smoke-release.sh --debug-installers` only for faster local iteration.

| Smoke | Result | Evidence |
|---|---|---|
| Folder happy path | TODO | Attach `scripts/smoke-folder-happy-path.sh` output; then install app, add a real media folder, search a known phrase, and capture the inline timestamp. |
| YouTube | TODO | Attach `scripts/smoke-youtube-source.sh --index-first --index-count 5` output and, when network is allowed, `scripts/smoke-youtube-source.sh --live --fetch-first --index-first --index-count 5 --max 5 --clip-duration-sec 12`; then add the live `https://www.youtube.com/@karpathy` channel in the installed app, search a Karpathy topic, and capture top result evidence. |
| Hotkey overlay | TODO | Attach `scripts/smoke-hotkey-overlay.sh` output and, on macOS installed builds, `scripts/smoke-installed-hotkey-macos.sh --dmg <release.dmg>` or `scripts/smoke-release.sh --installed-hotkey` output including `installed_hotkey_smoke`; if synthetic keypresses do not reach macOS global shortcuts, run `scripts/smoke-release.sh --installed-hotkey-manual` and press the physical hotkey. Then search and capture Enter opening the main window deep-linked. |
| Daemon survives close | TODO | Attach `scripts/smoke-installed-app.sh --dry-run` and `scripts/smoke-installed-app.sh` output proving installed daemon `/health`, settings persistence, inference mode persistence, and installed folder source discovery/queueing via the structured `installed_app_smoke` line; then close main window, confirm hotkey/indexing continue, reopen with state intact. |
| macOS signing/notarization | TODO | For public macOS release candidates, run `scripts/smoke-release.sh --signed-artifacts` or `scripts/build-installers.sh --require-signing` so missing signing/notarization prerequisites fail before packaging; attach `scripts/smoke-macos-signing.sh --dmg <release.dmg>` output. |
| Restart resilience | TODO | Attach `scripts/smoke-restart-resilience.sh` output; then force-quit mid-indexing on an installed build and confirm resume/no duplicate item indexing. |
| Search latency | TODO | Attach `scripts/smoke-search-latency.sh` output; requires p50 < 30ms and p99 < 100ms on 100 indexed items / 20 queries. |
| API model gates | TODO | With disposable provider keys configured, verify OpenAI transcription, Gemini `gemini-2.5-flash` audio fallback, and Gemini Embedding 2 against fixture media; attach provider/model IDs, transcript chunk count, embedding dimensions, and any provider error body. Historical local model smokes may be attached as optional future-track evidence, but they are not v1 release blockers. |
| Boot persistence macOS | TODO | Attach `scripts/smoke-daemon-autostart-macos.sh --dmg <release.dmg>` output including `daemon_autostart_smoke platform=macos` for installed-binary LaunchAgent install/uninstall roundtrip; before reboot, run `scripts/smoke-boot-persistence-macos.sh --dry-run --timeout 30` to record the exact post-login check; after reboot/login, run `scripts/smoke-boot-persistence-macos.sh --timeout 30` and attach output plus a hotkey screenshot before manually opening the app. |
EOF
  echo "Wrote manual smoke checklist to $REPORT_PATH"
fi
