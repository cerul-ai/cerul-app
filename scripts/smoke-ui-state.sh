#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "loadDesktopStore" apps/desktop/src/lib/uiStore.ts
rg -qF "ui-state.json" apps/desktop/src/lib/uiStore.ts
rg -qF "persistLastRoute" apps/desktop/src/lib/uiStore.ts
rg -qF "persistSidebarCollapsed" apps/desktop/src/lib/uiStore.ts
rg -qF "chunkId?: string | null" apps/desktop/src/lib/uiStore.ts
rg -qF "cerul.uiState.v1" apps/desktop/src/lib/uiStore.ts
rg -qF "loadPersistedUiState" apps/desktop/src
rg -qF "restorePersistedRoute" apps/desktop/src
rg -qF "setSelectedChunkId" apps/desktop/src
rg -qF "search.set(\"chunkId\", params.chunkId)" apps/desktop/src
rg -qF "toggleSidebarCollapsed" apps/desktop/src
rg -qF "sidebarCollapsed" apps/desktop/src
rg -qF "Collapse sidebar" apps/desktop/src
rg -qF "Expand sidebar" apps/desktop/src
rg -qF '.rail[data-collapsed="true"]' apps/desktop/src/styles/app.css
rg -qF ".rail-collapse" apps/desktop/src/styles/app.css
rg -qF "mainWindow.on(\"close\"" apps/electron-shell/src/main.ts
rg -qF "shouldCloseToTray().then" apps/electron-shell/src/main.ts
rg -qF "event.preventDefault()" apps/electron-shell/src/main.ts
rg -qF "mainWindow?.hide()" apps/electron-shell/src/main.ts
rg -qF "settingBoolean(await readApiSettings(), \"close_to_tray\", true)" apps/electron-shell/src/main.ts
rg -qF "scripts/smoke-ui-state.sh" scripts/smoke.sh

echo "ui_state_smoke store=electron_desktop_store fallback=localStorage last_route=persisted chunk_route=persisted sidebar_collapsed=persisted close_to_tray=native"
