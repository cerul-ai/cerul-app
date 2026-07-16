#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'if (!hasSources && apiStatus === "online")' apps/desktop/src
rg -qF 't("home.emptyHero.eyebrow")' apps/desktop/src
rg -qF 't("home.emptyHero.title")' apps/desktop/src
rg -qF 't("home.emptyHero.body")' apps/desktop/src
rg -qF 't("home.emptyHero.dragTitle")' apps/desktop/src
rg -qF 't("home.emptyHero.followYoutube")' apps/desktop/src
rg -qF '"home.emptyHero.title": "Turn video into searchable memory."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.emptyHero.dragTitle": "Drag a folder of videos here"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.emptyHero.followYoutube": "Follow a YouTube channel"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.searchPlaceholder": "Search any sentence from meetings, podcasts, or interviews..."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "const searchDisabled = hasSources && indexedCount === 0" apps/desktop/src
rg -qF "function SearchFirstPendingState(" apps/desktop/src/screens/home.tsx
rg -qF 'activeJobs.length > 0 &&' apps/desktop/src/screens/home.tsx
rg -qF '(apiStatus === "online" || pendingHomeFixture)' apps/desktop/src/screens/home.tsx
rg -qF 'className="search-first-examples"' apps/desktop/src/screens/home.tsx
rg -qF '.home-search-first-pending' apps/desktop/src/styles/selected-ui.css
rg -qF '"home.searchLockedPlaceholder": "Search when complete"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.lockedHint": "Search when complete."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"home.summary": "{count} media · {runtime} ready · {sources} sources"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"firstRun.indexing.body": "Search when complete."' apps/desktop/src/lib/i18n-catalog.ts
! rg -qF 't("home.emptyHero.costBadge")' apps/desktop/src/screens/home.tsx
! rg -qF 'jobs-ledger-local' apps/desktop/src/dialogs/jobs-sheet.tsx
! rg -qF 'indexing-strip__cost' apps/desktop/src/components/indexing-strip.tsx
! rg -qF 't("localModel.prop.local")' apps/desktop/src/components/local-model-consent.tsx
! rg -qF 't("localModel.prop.free")' apps/desktop/src/components/local-model-consent.tsx
rg -qF 'className={searchDisabled ? "search-wrap disabled" : "search-wrap"}' apps/desktop/src
rg -qF 'function RankingPreferenceMenu(' apps/desktop/src/screens/results.tsx
! rg -qF '<select value={rankingPreference}' apps/desktop/src/screens/results.tsx
rg -qF 'z-index:3; overflow:visible' apps/desktop/src/styles/selected-ui.css
rg -qF 'top:auto; bottom:calc(100% + 8px)' apps/desktop/src/styles/selected-ui.css
rg -qF "disabled={searchDisabled}" apps/desktop/src
rg -qF "function handleGlobalKeyDown(event: globalThis.KeyboardEvent)" apps/desktop/src
rg -qF 'settingString(data.settings, "hotkey_new_source", NEW_SOURCE_DEFAULT_HOTKEY)' apps/desktop/src
rg -qF "acceleratorMatchesEvent(newSourceHotkey, event)" apps/desktop/src
rg -qF "shouldIgnoreNewSourceShortcut(event.target)" apps/desktop/src
rg -qF "setShowAddSource(true)" apps/desktop/src
rg -qF ".search-wrap.disabled .search-input" apps/desktop/src/styles/ui.css
rg -qF ".home-status-line" apps/desktop/src/styles/app.css

echo "home_ui_smoke empty_state=add_source_cta pending_state=search_first indexing_only_search=disabled indexed_search=enabled cmd_n_add_source=enabled api_first_copy=enabled"
