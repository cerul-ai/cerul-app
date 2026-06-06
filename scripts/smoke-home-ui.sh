#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'if (!hasSources && apiStatus === "online")' apps/desktop/src
rg -qF 't("home.empty.addFirst")' apps/desktop/src
rg -qF 't("home.empty.title")' apps/desktop/src
rg -qF 't("home.empty.body")' apps/desktop/src
rg -qF '"home.empty.addFirst": "Add your first source"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.empty.title": "Nothing to search yet."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.empty.body": "Add a folder of videos, follow a YouTube channel, or connect a feed. Cerul indexes transcripts first, then adds remote API or local model retrieval."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.searchPlaceholder": "Search any sentence from meetings, podcasts, or interviews..."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "const searchDisabled = hasSources && indexedCount === 0" apps/desktop/src
rg -qF '"home.searchLockedPlaceholder": "Search unlocks after the first item is indexed"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.lockedHint": "Cerul is indexing your first sources. Search becomes available as soon as one item is ready."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF 'className={searchDisabled ? "search-wrap disabled" : "search-wrap"}' apps/desktop/src
rg -qF "disabled={searchDisabled}" apps/desktop/src
rg -qF "function handleGlobalKeyDown(event: globalThis.KeyboardEvent)" apps/desktop/src
rg -qF "event.key.toLowerCase() === \"n\"" apps/desktop/src
rg -qF "setShowAddSource(true)" apps/desktop/src
rg -qF ".search-wrap.disabled .search-input" apps/desktop/src/styles/ui.css
rg -qF ".home-status-line" apps/desktop/src/styles/app.css

echo "home_ui_smoke empty_state=add_source_cta indexing_only_search=disabled indexed_search=enabled cmd_n_add_source=enabled api_first_copy=enabled"
