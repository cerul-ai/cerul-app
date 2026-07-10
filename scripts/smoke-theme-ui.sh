#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'DEFAULT_THEME_PREFERENCE = "Light"' apps/desktop/src/lib/settings-helpers.ts
rg -qF 'DEFAULT_THEME_PREFERENCE' apps/desktop/src/App.tsx apps/desktop/src/OverlayApp.tsx apps/desktop/src/screens/settings.tsx
rg -qF 'settingString(await readApiSettings(), "theme", "Light")' apps/electron-shell/src/main.ts
rg -qF 'data-theme="light" data-theme-preference="light"' apps/desktop/index.html apps/desktop/overlay.html
rg -qF "function resolveThemePreference" apps/desktop/src
rg -qF "window.matchMedia(\"(prefers-color-scheme: light)\")" apps/desktop/src
rg -qF "root.dataset.theme = resolvedTheme" apps/desktop/src
rg -qF "root.dataset.themePreference = themePreference.toLowerCase()" apps/desktop/src
rg -qF "media?.addEventListener(\"change\", applyTheme)" apps/desktop/src
rg -qF "values={[\"System\", \"Light\", \"Dark\"]}" apps/desktop/src
rg -qF "[data-theme=\"light\"] {" apps/desktop/src/styles/tokens.css
rg -qF "[data-theme=\"dark\"] {" apps/desktop/src/styles/tokens.css
rg -qF -- "--accent:        #a85a28;" apps/desktop/src/styles/tokens.css
rg -qF -- "--accent:        #d99a62;" apps/desktop/src/styles/tokens.css
rg -qF ".app {" apps/desktop/src/styles/app.css
rg -qF ".bridge {" apps/desktop/src/styles/bridge.css
rg -qF -- "--ring: 0 0 0 3px rgba(168, 90, 40, 0.18);" apps/desktop/public/menubar.html
rg -qF 'event.stopPropagation();' apps/desktop/src/components/bridge.tsx
rg -qF 'onMouseDown={(event) => event.preventDefault()}' apps/desktop/src/components/bridge.tsx
rg -qF 'if (next === searchRankingPreference)' apps/desktop/src/App.tsx
rg -qF '".scrim, .account-pop, .menu, .bridge-menu' apps/desktop/src/App.tsx apps/desktop/src/screens/item-detail.tsx
! rg -qF 'rgba(28,40,60' apps/desktop/src/styles apps/desktop/public/brand/dmg/dmg-background-source.html
! rg -qF '.rail-dl-pill' apps/desktop/src
rg -qF "[data-theme=\"dark\"] .segmented button.active" apps/desktop/src/styles/ui.css
rg -qF "scripts/smoke-theme-ui.sh" scripts/smoke.sh

echo "theme_ui_smoke default=light setting=system_light_dark root_data_theme=enabled system_preference_listener=enabled light_tokens=enabled"
