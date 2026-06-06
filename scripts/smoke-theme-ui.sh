#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const themePreference = settingString(data.settings, \"theme\", \"Dark\")" apps/desktop/src
rg -qF "function resolveThemePreference" apps/desktop/src
rg -qF "window.matchMedia(\"(prefers-color-scheme: light)\")" apps/desktop/src
rg -qF "root.dataset.theme = resolvedTheme" apps/desktop/src
rg -qF "root.dataset.themePreference = themePreference.toLowerCase()" apps/desktop/src
rg -qF "media?.addEventListener(\"change\", applyTheme)" apps/desktop/src
rg -qF "values={[\"System\", \"Light\", \"Dark\"]}" apps/desktop/src
rg -qF "[data-theme=\"light\"] {" apps/desktop/src/styles/tokens.css
rg -qF "[data-theme=\"dark\"] {" apps/desktop/src/styles/tokens.css
rg -qF -- "--accent:        #4d6fa6;" apps/desktop/src/styles/tokens.css
rg -qF -- "--accent:        #8fb0d8;" apps/desktop/src/styles/tokens.css
rg -qF ".app {" apps/desktop/src/styles/app.css
rg -qF ".rail {" apps/desktop/src/styles/app.css
rg -qF "[data-theme=\"dark\"] .segmented button.active" apps/desktop/src/styles/ui.css
rg -qF "scripts/smoke-theme-ui.sh" scripts/smoke.sh

echo "theme_ui_smoke setting=system_light_dark root_data_theme=enabled system_preference_listener=enabled light_tokens=enabled"
