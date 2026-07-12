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
rg -qF 't("sourceRow.retryDiscovery")' apps/desktop/src
rg -qF 't("sourceRow.remove")' apps/desktop/src
rg -qF "/retry-discovery" apps/desktop/src/lib/api.ts
rg -qF ".source-error-toggle" apps/desktop/src/styles/extensions.css
rg -qF ".source-error-panel" apps/desktop/src/styles/extensions.css
rg -qF 'className="page wide p3-page saved-p3-page"' apps/desktop/src/screens/moments.tsx
rg -qF 'className="saved-collection-grid"' apps/desktop/src/screens/moments.tsx
rg -qF 'items={visibleItems}' apps/desktop/src/App.tsx
rg -qF 'className="page wide p3-page sources-p3-page"' apps/desktop/src/screens/sources.tsx
rg -qF 'className="connector-grid"' apps/desktop/src/screens/sources.tsx
rg -qF 'className="connector-activity card"' apps/desktop/src/screens/sources.tsx
rg -qF 'className="connector-detail card"' apps/desktop/src/screens/sources.tsx
rg -qF '.p3-workspace {' apps/desktop/src/styles/selected-ui.css
rg -qF '.saved-collection-grid {' apps/desktop/src/styles/selected-ui.css
rg -qF '.connector-grid {' apps/desktop/src/styles/selected-ui.css

echo "sources_ui_smoke p3_saved_console=enabled p3_connectors=enabled activity_timeline=enabled empty_state=enabled source_error_expandable=enabled fix_remove_actions=enabled"
