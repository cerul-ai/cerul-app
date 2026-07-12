#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo metadata --no-deps --format-version 1 >/dev/null
bash scripts/smoke-core-banner-ui.sh
bash scripts/smoke-onboarding-ui.sh
bash scripts/smoke-home-ui.sh
bash scripts/smoke-first-run-ui.sh
bash scripts/smoke-jobs-ui.sh
bash scripts/smoke-notifications.sh
bash scripts/smoke-tray.sh
bash scripts/smoke-results-ui.sh
bash scripts/smoke-add-source-ui.sh
bash scripts/smoke-audio-image-sources.sh
bash scripts/smoke-sources-ui.sh
bash scripts/smoke-detail-ui.sh
bash scripts/smoke-shares-ui.sh
bash scripts/smoke-library-ui.sh
bash scripts/smoke-item-detail-ui.sh
bash scripts/smoke-confirm-dialog-ui.sh
bash scripts/smoke-brand-assets-ui.sh
bash scripts/smoke-settings-ui.sh
bash scripts/smoke-theme-ui.sh
bash scripts/smoke-ui-state.sh
cargo check --workspace
pnpm typecheck
pnpm build
