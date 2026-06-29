#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'export type CoreBannerAction = "retry" | "restart"' apps/desktop/src/lib/types.ts
rg -qF "function restartCoreConnection" apps/desktop/src
rg -qF "onAction={restartCoreConnection}" apps/desktop/src
rg -qF "const [elapsedMs, setElapsedMs]" apps/desktop/src
rg -qF "const UNRESPONSIVE_MS = 10_000" apps/desktop/src
rg -qF "elapsedMs >= UNRESPONSIVE_MS" apps/desktop/src
rg -qF 't("coreBanner.unresponsive")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.retrying")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.retry")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("shell.coreStarting")' apps/desktop/src/App.tsx
rg -qF 't("shell.coreUnresponsive")' apps/desktop/src/App.tsx
rg -qF "CoreStatusToast" apps/desktop/src/App.tsx
rg -qF 'show={view !== "settings" && coreLevel === "unresponsive"}' apps/desktop/src/App.tsx
rg -qF '"coreBanner.starting": "Core starting"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"coreBanner.unresponsive": "Core offline"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "role=\"status\"" apps/desktop/src
rg -qF 'className="settings-core-status"' apps/desktop/src/App.tsx
rg -qF '.rail-status-dot[data-level="starting"]' apps/desktop/src/styles/app.css
rg -qF '.rail-status-dot[data-level="unresponsive"]' apps/desktop/src/styles/app.css
rg -qF '.core-toast[data-show="true"]' apps/desktop/src/styles/extensions.css
rg -qF ".core-toast button" apps/desktop/src/styles/extensions.css

echo "core_banner_ui_smoke startup_spinner=enabled unresponsive_after_10s=enabled retry_action=enabled settings_sidebar_status=enabled error_detail=tooltip"
