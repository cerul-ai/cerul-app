#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "function setupTray" apps/electron-shell/src/main.ts
rg -qF "new Tray" apps/electron-shell/src/main.ts
rg -qF "Open Cerul" apps/electron-shell/src/main.ts
rg -qF "Search Overlay" apps/electron-shell/src/main.ts
rg -qF "case \"update_tray_idle_status\"" apps/electron-shell/src/main.ts
rg -qF "case \"update_tray_indexing_status\"" apps/electron-shell/src/main.ts
rg -qF "Cerul · indexing" apps/electron-shell/src/main.ts
rg -qF 'Cerul · ${args.indexed' apps/electron-shell/src/main.ts
rg -qF "scripts/smoke-tray.sh" scripts/smoke.sh

echo "tray_smoke dynamic_status=indexing_idle pause_resume=setting_backed notifications=progress_events"
