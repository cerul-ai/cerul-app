#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'export function SharesScreen' apps/desktop/src/screens/shares.tsx
rg -qF '"shares-workspace empty-inspector"' apps/desktop/src/screens/shares.tsx
rg -qF '"shares-inspector shares-inspector--empty"' apps/desktop/src/screens/shares.tsx
rg -qF '.shares-workspace { min-height:0; flex:1; display:grid; grid-template-columns:clamp(168px,12vw,190px) minmax(0,1fr) clamp(280px,21vw,320px)' apps/desktop/src/styles/selected-ui.css
rg -qF 'align-items:stretch; gap:0;' apps/desktop/src/styles/selected-ui.css
rg -qF '.shares-filter-rail { padding:12px 11px; border-right:1px solid var(--line); }' apps/desktop/src/styles/selected-ui.css
rg -qF '.shares-inspector { align-self:stretch; padding:13px; border-left:1px solid var(--line); }' apps/desktop/src/styles/selected-ui.css
rg -qF 'onNavigate("shares")' apps/desktop/src/components/bridge.tsx
rg -qF 'view === "shares"' apps/desktop/src/App.tsx
rg -qF 'recordManagedShare(published, {' apps/desktop/src/screens/item-detail.tsx
rg -qF 'cloudClient.revokeShare(accessToken, share.id)' apps/desktop/src/screens/shares.tsx
rg -qF 'writeClipboardText(share.share_url)' apps/desktop/src/screens/shares.tsx
rg -qF 'window.open(selectedShare.share_url' apps/desktop/src/screens/shares.tsx
rg -qF '.shares-workspace' apps/desktop/src/styles/selected-ui.css
rg -qF '.share-ledger-row.active' apps/desktop/src/styles/selected-ui.css
rg -qF '"nav.shares": "分享管理"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"nav.shares": "Share management"' apps/desktop/src/lib/i18n-catalog.ts

echo "shares_ui_smoke entry=avatar_secondary layout=single_three_pane_workbench ledger=device_local actions=copy_preview_revoke card=S2 external=A2 single_detail_share_action=copy_citation"
