#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'status: "error"' apps/desktop/src
rg -qF "sourceError(record, status, t)" apps/desktop/src
rg -qF "source.status === \"error\"" apps/desktop/src
rg -qF "const [errorExpanded, setErrorExpanded]" apps/desktop/src
rg -qF "source-error-toggle" apps/desktop/src
rg -qF "source-error-panel" apps/desktop/src
rg -qF '"sourceRow.errorTitle": "Source needs attention"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF 't("sourceRow.fix")' apps/desktop/src
rg -qF 't("sourceRow.remove")' apps/desktop/src
rg -qF ".source-error-toggle" apps/desktop/src/styles/extensions.css
rg -qF ".source-error-panel" apps/desktop/src/styles/extensions.css

echo "sources_ui_smoke empty_state=enabled source_error_expandable=enabled fix_remove_actions=enabled"
