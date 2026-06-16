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
rg -qF 't("coreBanner.restarting")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.restart")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("shell.coreStarting")' apps/desktop/src/App.tsx
rg -qF 't("shell.coreUnresponsive")' apps/desktop/src/App.tsx
rg -qF "CoreStatusToast" apps/desktop/src/App.tsx
rg -qF '"coreBanner.starting": "Cerul Core is starting up..."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"coreBanner.unresponsive": "Cerul Core is unresponsive."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "role=\"status\"" apps/desktop/src
rg -qF '.rail-status-dot[data-level="starting"]' apps/desktop/src/styles/app.css
rg -qF '.rail-status-dot[data-level="unresponsive"]' apps/desktop/src/styles/app.css
rg -qF '.core-toast[data-show="true"]' apps/desktop/src/styles/extensions.css
rg -qF ".core-toast button" apps/desktop/src/styles/extensions.css

echo "core_banner_ui_smoke startup_spinner=enabled unresponsive_after_10s=enabled restart_action=enabled error_detail=enabled"
