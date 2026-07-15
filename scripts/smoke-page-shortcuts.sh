#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'accelerator: "CommandOrControl+F"' apps/electron-shell/src/main.ts
rg -qF 'type: "find"' apps/electron-shell/src/main.ts
rg -qF 'command.type === "new_source" || command.type === "find"' apps/desktop/src/lib/desktopHost.ts
rg -qF 'view === "library"' apps/desktop/src/App.tsx
rg -qF '"cerul:focus-home-search"' apps/desktop/src/App.tsx
rg -qF '"cerul:focus-library-search"' apps/desktop/src/App.tsx
rg -qF '"cerul:focus-jobs-search"' apps/desktop/src/App.tsx
rg -qF '"cerul:focus-bridge-search"' apps/desktop/src/App.tsx
rg -qF 'event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey' apps/desktop/src/App.tsx
rg -qF 'window.addEventListener("cerul:focus-bridge-search", focusSearch)' apps/desktop/src/components/bridge.tsx
rg -qF 'window.addEventListener("cerul:focus-home-search", focusSearch)' apps/desktop/src/screens/home.tsx
rg -qF 'window.addEventListener("cerul:focus-library-search", focusSearch)' apps/desktop/src/screens/library.tsx
rg -qF 'event.key !== "ArrowDown" && event.key !== "ArrowUp"' apps/desktop/src/screens/library.tsx
rg -qF 'window.addEventListener("cerul:focus-jobs-search", focusSearch)' apps/desktop/src/dialogs/jobs-sheet.tsx
rg -qF 'event.key === "ArrowDown" || event.key === "ArrowUp"' apps/desktop/src/dialogs/jobs-sheet.tsx

echo "page_shortcuts_smoke scheme=macos_native cmd_f=native_menu arrows=library_results_jobs space=native_preview esc=task_page"
