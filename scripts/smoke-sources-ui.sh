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
rg -qF 'const visibleMoments = view === "videos" ? [] : moments' apps/desktop/src/screens/moments.tsx
rg -qF '{ id: "review", label: t("moments.p3.review"), count: moments.length }' apps/desktop/src/screens/moments.tsx
rg -qF 'items={visibleItems}' apps/desktop/src/App.tsx
rg -qF 'className="page wide p3-page sources-p3-page"' apps/desktop/src/screens/sources.tsx
rg -qF 'type ConnectorKind = "bilibili" | "youtube" | "local" | "podcast" | "web"' apps/desktop/src/screens/sources.tsx
rg -qF 'className="p3-main-scroll source-groups"' apps/desktop/src/screens/sources.tsx
rg -qF 'className={expanded ? "source-group card expanded" : "source-group card"}' apps/desktop/src/screens/sources.tsx
rg -qF 'attentionKindSignature' apps/desktop/src/screens/sources.tsx
rg -qF 'return bAttention - aAttention' apps/desktop/src/screens/sources.tsx
rg -qF 'isHostOrSubdomain(host, "b23.tv")' apps/desktop/src/screens/sources.tsx
rg -qF 'sources.length > 0 && (view === "all" || group.sources.length > 0)' apps/desktop/src/screens/sources.tsx
rg -qF 'view === "history" ? (' apps/desktop/src/screens/sources.tsx
rg -qF 'className="connector-timeline"' apps/desktop/src/screens/sources.tsx
rg -qF '(b.lastPolledEpoch ?? 0) - (a.lastPolledEpoch ?? 0)' apps/desktop/src/screens/sources.tsx
rg -qF '["shorts", "live", "embed", "c", "user", "channel"]' apps/desktop/src/lib/sources.ts
rg -qF 'source.type === "podcast" && parts.length > 0' apps/desktop/src/lib/sources.ts
rg -qF 'sourceConnectorDisplayName(source, t("sources.p3.unnamed"))' apps/desktop/src/screens/sources.tsx
rg -qF '.p3-workspace {' apps/desktop/src/styles/selected-ui.css
rg -qF '.saved-collection-grid {' apps/desktop/src/styles/selected-ui.css
rg -qF '.source-group-list .source-row {' apps/desktop/src/styles/selected-ui.css
rg -qF '.source-group.expanded { height:auto; max-height:none; }' apps/desktop/src/styles/selected-ui.css

echo "sources_ui_smoke layout=C4_type_accordion groups=bilibili_youtube_local_podcast_web short_link=b23.tv path_identity=youtube_short_live_channel_and_podcast_feed empty_state=restored history=timeline anomaly=auto_expanded_and_sorted_first source_error_expandable=enabled fix_remove_actions=enabled"
