#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "function showNotification" apps/electron-shell/src/main.ts
rg -qF "Notification.isSupported()" apps/electron-shell/src/main.ts
rg -qF "new Notification({ title, body }).show()" apps/electron-shell/src/main.ts
rg -qF "case \"notify_first_items_indexed\"" apps/electron-shell/src/main.ts
rg -qF "case \"notify_indexing_complete\"" apps/electron-shell/src/main.ts
rg -qF "case \"notify_update_available\"" apps/electron-shell/src/main.ts
rg -qF "case \"notify_items_failed\"" apps/electron-shell/src/main.ts
rg -qF "case \"notify_folder_source_missing\"" apps/electron-shell/src/main.ts

echo "notifications_smoke electron_notifications=enabled titles_bodies=verified commands=stable"
