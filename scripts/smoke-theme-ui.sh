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
rg -qF 'onKeyDown={onSearchEscape}' apps/desktop/src/components/bridge.tsx
rg -qF 'onMouseDown={(event) => event.preventDefault()}' apps/desktop/src/components/bridge.tsx
rg -qF 'onRankingPreferenceChange(preference, value)' apps/desktop/src/components/bridge.tsx
rg -qF 'hotkeyLabel={formatHotkeyLabel(globalHotkey)}' apps/desktop/src/App.tsx
rg -qF 'if (next === searchRankingPreference && draftQuery === query)' apps/desktop/src/App.tsx
rg -qF 'searchVisible={view !== "home" && view !== "onboarding"}' apps/desktop/src/App.tsx
rg -qF 'animation: cerul-spin 1s linear infinite;' apps/desktop/src/styles/bridge.css
rg -qF 'rgb(217 154 98 / 35%)' apps/desktop/src/styles/extensions.css
rg -qF -- '--bridge-row-h: 51px;' apps/desktop/src/styles/app.css
rg -qF 'var(--bridge-row-h)' apps/desktop/src/styles/app.css
rg -qF '".scrim, .account-pop, .menu, .bridge-menu' apps/desktop/src/App.tsx apps/desktop/src/screens/item-detail.tsx
! rg -qF 'rgba(28,40,60' apps/desktop/src/styles apps/desktop/public/brand/dmg/dmg-background-source.html
! rg -qF '.rail-dl-pill' apps/desktop/src
rg -qF "[data-theme=\"dark\"] .segmented button.active" apps/desktop/src/styles/ui.css
rg -qF "scripts/smoke-theme-ui.sh" scripts/smoke.sh
rg -qF 'className={`cerul-launch splash-playing' apps/desktop/src/components/launch-splash.tsx
rg -qF 'Where video becomes citable' apps/desktop/src/components/launch-splash.tsx
rg -qF 'window.matchMedia("(prefers-reduced-motion: reduce)").matches' apps/desktop/src/components/launch-splash.tsx
rg -qF '/brand/svg/cerul-icon-paper.svg' apps/desktop/src/components/launch-splash.tsx
rg -qF '.cerul-launch__tagline' apps/desktop/src/styles/selected-ui.css

echo "theme_ui_smoke default=light setting=system_light_dark root_data_theme=enabled system_preference_listener=enabled light_tokens=warm_glass dark_tokens=cool_graphite launch=A5_T3"
