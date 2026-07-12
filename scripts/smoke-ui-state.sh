#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "loadDesktopStore" apps/desktop/src/lib/uiStore.ts
rg -qF "ui-state.json" apps/desktop/src/lib/uiStore.ts
rg -qF "persistLastRoute" apps/desktop/src/lib/uiStore.ts
rg -qF "persistOnboardingCompleted" apps/desktop/src/lib/uiStore.ts
rg -qF "playbackChunkId?: string | null" apps/desktop/src/lib/uiStore.ts
rg -qF "hasCompletedOnboarding?: boolean" apps/desktop/src/lib/uiStore.ts
rg -qF "cerul.uiState.v1" apps/desktop/src/lib/uiStore.ts
rg -qF "loadPersistedUiState" apps/desktop/src
rg -qF "restorePersistedRoute" apps/desktop/src
rg -qF 'route.view === "result-detail" ? "item-detail" : route.view' apps/desktop/src/App.tsx
rg -qF 'route.view === "result-detail" ? "results" : route.origin' apps/desktop/src/App.tsx
rg -qF 'search.set("origin", params.origin)' apps/desktop/src/lib/route.ts
rg -qF "setSelectedPlaybackChunkId" apps/desktop/src
rg -qF "search.set(\"playbackChunkId\", params.playbackChunkId)" apps/desktop/src
rg -qF "!state.hasCompletedOnboarding && !state.lastRoute && !window.location.hash" apps/desktop/src
rg -qF "void persistOnboardingCompleted(true)" apps/desktop/src
rg -qF "createMainBrowserWindow({" apps/electron-shell/src/main.ts
rg -qF "shouldCloseToTray," apps/electron-shell/src/main.ts
rg -qF 'window.on("close", (event) => {' apps/electron-shell/src/windows.ts
rg -qF "event.preventDefault()" apps/electron-shell/src/windows.ts
rg -qF "options.shouldCloseToTray().then" apps/electron-shell/src/windows.ts
rg -qF "window.hide()" apps/electron-shell/src/windows.ts
rg -qF "settingBoolean(await readApiSettings(), \"close_to_tray\", true)" apps/electron-shell/src/main.ts
rg -qF "scripts/smoke-ui-state.sh" scripts/smoke.sh

echo "ui_state_smoke store=electron_desktop_store fallback=localStorage last_route=persisted legacy_result_detail=migrated origin=persisted playback_chunk_route=persisted onboarding_completed=persisted close_to_tray=native"
