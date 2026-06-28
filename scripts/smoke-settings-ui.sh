#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'type SaveStatus = "idle" | "saving" | "saved" | "error"' apps/desktop/src
rg -qF "const controlsDisabled = apiStatus !== \"online\"" apps/desktop/src
rg -qF "async function saveSettings" apps/desktop/src
rg -qF "Saving..." apps/desktop/src
rg -qF "Settings saved" apps/desktop/src
rg -qF "Cerul Core is not reachable." apps/desktop/src
rg -qF "type DaemonStatus" apps/desktop/src
rg -qF 'invokeHostCommand<DaemonStatus>("daemon_status")' apps/desktop/src
rg -qF "daemonStatus" apps/desktop/src
rg -qF "async function saveStartAtLogin" apps/desktop/src
rg -qF "await installDaemon()" apps/desktop/src
rg -qF "await uninstallDaemon()" apps/desktop/src
rg -qF "start_at_login: result.installed" apps/desktop/src
rg -qF "Start at login is not available on this platform." apps/desktop/src
rg -qF "Global hotkey" apps/desktop/src
rg -qF "global_hotkey" apps/desktop/src
rg -qF "function ShortcutsSettings" apps/desktop/src/App.tsx
rg -qF "settings.section.shortcuts" apps/desktop/src
rg -qF "hotkey_new_source" apps/desktop/src apps/electron-shell/src/main.ts
rg -qF "hotkey_open_settings" apps/desktop/src apps/electron-shell/src/main.ts
rg -qF "hotkey_close_window" apps/desktop/src apps/electron-shell/src/main.ts
rg -qF "async function saveShortcut" apps/desktop/src/App.tsx
rg -qF "setGlobalHotkey(accelerator)" apps/desktop/src/App.tsx
rg -qF "sync_application_menu" apps/desktop/src apps/electron-shell/src/main.ts
rg -qF "subscribeDesktopMenuCommand" apps/desktop/src
rg -qF "onMenuCommand" apps/desktop/src apps/electron-shell/src/preload.ts
rg -qF "Pause in low-power mode" apps/desktop/src
rg -qF "Inference mode" apps/desktop/src
rg -qF "Remote API" apps/desktop/src
rg -qF "Local model" apps/desktop/src
rg -qF "Provider connections" apps/desktop/src
rg -qF "Explore models" apps/desktop/src
rg -qF "discoverProviderModels" apps/desktop/src
rg -qF "/providers/:id/models" crates/cerul-api/src/lib.rs
rg -qF "discover_provider_models" crates/cerul-api/src
rg -qF "Show in Finder" apps/desktop/src
rg -qF "Clear cache" apps/desktop/src
rg -qF "API key for remote access" apps/desktop/src
rg -qF "Changes take effect after restart." apps/desktop/src
rg -qF "Authorization: Bearer" apps/desktop/src
rg -qF "Anonymous usage counters, off by default" apps/desktop/src
rg -qF "Open logs folder" apps/desktop/src
rg -qF "Check for updates" apps/desktop/src
rg -qF 'phase: "installing"' apps/electron-shell/src/main.ts
rg -qF "installDesktopUpdate();" apps/electron-shell/src/main.ts
rg -qF "isQuitting = true;" apps/electron-shell/src/main.ts
rg -qF "updater.quitAndInstall(false, true)" apps/electron-shell/src/main.ts
rg -qF "shell.updateInstalling" apps/desktop/src
rg -qF "Commit" apps/desktop/src
rg -qF "Build date" apps/desktop/src
rg -qF "function revealDataDirectory" apps/desktop/src
rg -qF "function revealLogsDirectory" apps/desktop/src
rg -qF "function clearCacheDirectory" apps/desktop/src
rg -qF "case \"reveal_data_directory\"" apps/electron-shell/src/main.ts
rg -qF "case \"reveal_logs_directory\"" apps/electron-shell/src/main.ts
rg -qF "case \"clear_cache\"" apps/electron-shell/src/main.ts
rg -qF "case \"daemon_status\"" apps/electron-shell/src/main.ts
rg -qF "app.getLoginItemSettings({ args: loginItemArgs() })" apps/electron-shell/src/main.ts
rg -qF "app.setLoginItemSettings" apps/electron-shell/src/main.ts
rg -qF "linuxAutostartPath()" apps/electron-shell/src/main.ts
rg -qF "case \"storage_locations\"" apps/electron-shell/src/main.ts
rg -qF "disabled={controlsDisabled}" apps/desktop/src
rg -qF "disabled={disabled}" apps/desktop/src
rg -qF "saveChipClass" apps/desktop/src/App.tsx
rg -qF 'role="status" aria-live="polite"' apps/desktop/src/App.tsx
rg -qF ".chip.success" apps/desktop/src/styles/ui.css
rg -qF ".chip.danger" apps/desktop/src/styles/ui.css
rg -qF ".settings-inline-action" apps/desktop/src/styles/extensions.css
rg -qF ".settings-stack-control" apps/desktop/src/styles/extensions.css
rg -qF ".setting-row-control > .select" apps/desktop/src/styles/extensions.css
rg -qF ".select:disabled" apps/desktop/src/styles/ui.css

echo "settings_ui_smoke disconnected_controls=disabled autosave_status=enabled start_at_login=daemon_status_synced shortcuts_directory=configurable native_menu_shortcuts=synced global_hotkey=configurable inference_mode=remote_or_local provider_controls=guarded model_discovery=enabled storage_actions=enabled updater_auto_install=enabled advanced_actions=remote_auth about_actions=enabled"
