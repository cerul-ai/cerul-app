#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF 'export function SharesScreen' apps/desktop/src/screens/shares.tsx
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

echo "shares_ui_smoke entry=avatar_secondary ledger=device_local actions=copy_preview_revoke card=S2 external=A2 single_detail_share_action=copy_citation"
