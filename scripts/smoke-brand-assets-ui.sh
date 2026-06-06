#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

test -s apps/desktop/public/brand/cerul-mark-light.svg
test -s apps/desktop/public/brand/cerul-mark-dark.svg
test -s apps/desktop/public/brand/cerul-mark-white.svg
test -s apps/desktop/public/brand/cerul-mark-color.svg
test -s apps/desktop/public/brand/cerul-icon-mac-1024.png
test -s apps/desktop/public/brand/cerul-menubarTemplate.png
test -s apps/desktop/public/brand/cerul-menubarTemplate@2x.png
test -s apps/desktop/public/brand/cerul-tray.png
test -s apps/desktop/public/brand/apple-touch-icon.png
test -s apps/desktop/public/brand/icon-192.png
test -s apps/desktop/public/brand/icon-512.png
test -s apps/desktop/public/brand/app-store-icon-1024.png
test -s apps/desktop/public/brand/cerul.icns
test -s apps/desktop/public/brand/cerul.ico
test -s apps/desktop/public/brand/dmg/dmg-background.png
test -s apps/desktop/public/brand/dmg/dmg-background@2x.png
test -s apps/desktop/public/brand/nsis/installerSidebar.bmp
test -s apps/desktop/public/brand/nsis/installerHeader.bmp

rg -qF "markLight: \"/brand/cerul-mark-light.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markDark: \"/brand/cerul-mark-dark.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markWhite: \"/brand/cerul-mark-white.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markColor: \"/brand/cerul-mark-color.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "function BrandMarkAsset" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markLight}" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markDark}" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markWhite}" apps/desktop/src/components/brand.tsx
rg -qF "function OverlayMark" apps/desktop/src/OverlayApp.tsx
rg -qF "className=\"overlay-mark\"" apps/desktop/src/OverlayApp.tsx
rg -qF "function BrandLogo" apps/desktop/src
rg -qF "function BrandMark" apps/desktop/src
rg -qF ".brandmark" apps/desktop/src/styles/app.css
rg -qF ".brandmark-img-dark" apps/desktop/src/styles/app.css
rg -qF ".onb-brand .logo-lockup" apps/desktop/src/styles/app.css
rg -qF ".mobilebar .brandmark" apps/desktop/src/styles/app.css
rg -qF "rel=\"apple-touch-icon\"" apps/desktop/index.html
rg -qF "rel=\"apple-touch-icon\"" apps/desktop/overlay.html
rg -qF "\"appId\": \"ai.cerul.desktop\"" apps/electron-shell/package.json
rg -qF "\"minimumSystemVersion\": \"10.15\"" apps/electron-shell/package.json
rg -qF "\"icon\": \"../desktop/public/brand/app-store-icon-1024.png\"" apps/electron-shell/package.json
rg -qF "function desktopAppIconPath" apps/electron-shell/src/main.ts
rg -qF "brand/app-store-icon-1024.png" apps/electron-shell/src/main.ts
rg -qF "function trayIconPath" apps/electron-shell/src/main.ts
rg -qF "brand/cerul-menubarTemplate.png" apps/electron-shell/src/main.ts
rg -qF "brand/icon-192.png" apps/electron-shell/src/main.ts
rg -qF "image.setTemplateImage(true)" apps/electron-shell/src/main.ts
rg -qF "\"background\": \"../desktop/public/brand/dmg/dmg-background.png\"" apps/electron-shell/package.json
rg -qF "\"oneClick\": false" apps/electron-shell/package.json
rg -qF "\"installerSidebar\": \"../desktop/public/brand/nsis/installerSidebar.bmp\"" apps/electron-shell/package.json
rg -qF "\"installerHeader\": \"../desktop/public/brand/nsis/installerHeader.bmp\"" apps/electron-shell/package.json
rg -qF "\"schemes\": [" apps/electron-shell/package.json
rg -qF "\"cerul-app\"" apps/electron-shell/package.json
rg -qF "\"from\": \"bin\"" apps/electron-shell/package.json
rg -qF "scripts/smoke-brand-assets-ui.sh" scripts/smoke.sh

echo "brand_assets_ui_smoke public_assets=present themed_logo=asset_backed themed_mark=asset_backed electron_icon=present tray_icon=mac_template_nonmac_color bundle_identifier=desktop macos_min=10.15 protocols=registered resources=declared"
