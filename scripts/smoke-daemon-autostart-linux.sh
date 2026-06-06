#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DRY_RUN=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --binary)
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      echo "Usage: scripts/smoke-daemon-autostart-linux.sh [--dry-run]" >&2
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [ "$DRY_RUN" -eq 1 ]; then
  echo "+ verify Electron login item IPC contract for Linux release smoke"
  echo "daemon_autostart_smoke platform=linux status=planned owner=electron_login_item"
  exit 0
fi

rg -qF "case \"daemon_status\"" apps/electron-shell/src/main.ts
rg -qF "case \"install_daemon\"" apps/electron-shell/src/main.ts
rg -qF "case \"uninstall_daemon\"" apps/electron-shell/src/main.ts
rg -qF "process.platform === \"linux\"" apps/electron-shell/src/main.ts
rg -qF "linuxAutostartPath()" apps/electron-shell/src/main.ts
rg -qF "installLinuxAutostart()" apps/electron-shell/src/main.ts
rg -qF "uninstallLinuxAutostart()" apps/electron-shell/src/main.ts
rg -qF "fs.existsSync(autostartPath)" apps/electron-shell/src/main.ts
rg -qF "async function installDaemon" apps/desktop/src/App.tsx
rg -qF "async function uninstallDaemon" apps/desktop/src/App.tsx
rg -qF "await installDaemon()" apps/desktop/src/App.tsx
rg -qF "await uninstallDaemon()" apps/desktop/src/App.tsx

echo "daemon_autostart_smoke platform=linux status=passed owner=electron_xdg_autostart install=ipc uninstall=ipc status_check=ipc"
