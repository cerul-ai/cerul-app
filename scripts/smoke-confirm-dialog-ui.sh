#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "type ConfirmOptions" apps/desktop/src
rg -qF "type ConfirmRequest" apps/desktop/src
rg -qF "type RequestConfirm" apps/desktop/src
rg -qF "function requestConfirm" apps/desktop/src
rg -qF "function resolveConfirm" apps/desktop/src
rg -qF "function ConfirmDialog" apps/desktop/src
rg -qF "aria-labelledby=\"confirm-title\"" apps/desktop/src
rg -qF "Delete selected items" apps/desktop/src
rg -qF "Remove source" apps/desktop/src
rg -qF "requestConfirm={requestConfirm}" apps/desktop/src
rg -qF ".confirm-dialog" apps/desktop/src/styles/extensions.css
rg -qF ".confirm-icon" apps/desktop/src/styles/extensions.css
rg -qF "scripts/smoke-confirm-dialog-ui.sh" scripts/smoke.sh
if rg -qF "window.confirm" apps/desktop/src; then
  echo "native window.confirm should not be used in App.tsx" >&2
  exit 1
fi

echo "confirm_dialog_ui_smoke app_confirm=enabled native_confirm=removed delete_item=guarded batch_delete=guarded source_remove=guarded"
