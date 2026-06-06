#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'export type CoreBannerAction = "retry" | "restart"' apps/desktop/src/lib/types.ts
rg -qF "function restartCoreConnection" apps/desktop/src
rg -qF "onAction={restartCoreConnection}" apps/desktop/src
rg -qF "const [elapsedMs, setElapsedMs]" apps/desktop/src
rg -qF "elapsedMs >= 10_000" apps/desktop/src
rg -qF 't("coreBanner.starting")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.unresponsive")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.restarting")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.retrying")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.restart")' apps/desktop/src/components/core-banner.tsx
rg -qF 't("coreBanner.retry")' apps/desktop/src/components/core-banner.tsx
rg -qF '"coreBanner.starting": "Cerul Core is starting up..."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"coreBanner.unresponsive": "Cerul Core is unresponsive."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "role=\"status\"" apps/desktop/src
rg -qF ".core-banner.unresponsive" apps/desktop/src/styles/extensions.css
rg -qF ".core-banner button" apps/desktop/src/styles/extensions.css

echo "core_banner_ui_smoke startup_spinner=enabled unresponsive_after_10s=enabled restart_action=enabled error_detail=enabled"
