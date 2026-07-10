#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

test -s apps/desktop/public/brand/svg/cerul-icon-graphite.svg
test -s apps/desktop/public/brand/svg/cerul-icon-graphite.svg
test -s apps/desktop/public/brand/svg/cerul-wordmark-light.svg
test -s apps/desktop/public/brand/svg/cerul-wordmark-dark.svg
test -s apps/desktop/public/brand/svg/cerul-wordmark-lockup-outline.svg
for legacy_mark in \
  apps/desktop/public/brand/cerul-mark-light.svg \
  apps/desktop/public/brand/cerul-mark-dark.svg \
  apps/desktop/public/brand/cerul-mark-white.svg \
  apps/desktop/public/brand/cerul-mark-color.svg \
  apps/desktop/public/brand/cerul-mark.svg \
  apps/desktop/public/brand/svg/cerul-mark.svg \
  apps/desktop/public/brand/svg/cerul-mark-mono-white.svg \
  apps/desktop/public/brand/svg/cerul-mark-mono-black.svg
do
  test -s "$legacy_mark"
  rg -qF "viewBox=\"0 0 1024 1024\"" "$legacy_mark"
done
rg -qF "width=\"1024\" height=\"1024\" rx=\"229\"" apps/desktop/public/brand/svg/cerul-wordmark-light.svg
rg -qF "width=\"1024\" height=\"1024\" rx=\"229\"" apps/desktop/public/brand/svg/cerul-wordmark-dark.svg
rg -qF "width=\"1024\" height=\"1024\" rx=\"229\"" apps/desktop/public/brand/svg/cerul-wordmark-lockup-outline.svg
! rg -qF "scale(0.1535)" apps/desktop/public/brand/svg/cerul-wordmark-light.svg apps/desktop/public/brand/svg/cerul-wordmark-dark.svg
! rg -qF "scale(0.374)" apps/desktop/public/brand/svg/cerul-wordmark-lockup-outline.svg
test -s apps/desktop/public/brand/cerul-icon-mac-1024.png
test -s apps/desktop/public/brand/cerul-menubarTemplate.png
test -s apps/desktop/public/brand/cerul-menubarTemplate@2x.png
test -s apps/desktop/public/brand/cerul-tray.png
test -s apps/desktop/public/brand/menubar/cerul-menubarTemplate-16.png
test -s apps/desktop/public/brand/menubar/cerul-menubarTemplate-32.png
test -s apps/desktop/public/brand/menubar/cerul-menubarTemplate-64.png
test -s apps/desktop/public/brand/tray/cerul-tray-16.png
test -s apps/desktop/public/brand/tray/cerul-tray-24.png
test -s apps/desktop/public/brand/tray/cerul-tray-32.png
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
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-light-1x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-light-2x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-dark-1x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-dark-2x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-lockup-light-1x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-lockup-light-2x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-lockup-dark-1x.png
test -s apps/desktop/public/brand/wordmark/cerul-wordmark-lockup-dark-2x.png

rg -qF "markLight: \"/brand/svg/cerul-icon-graphite.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markDark: \"/brand/svg/cerul-icon-paper.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markWhite: \"/brand/svg/cerul-icon-paper.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "markColor: \"/brand/svg/cerul-icon-graphite.svg\"" apps/desktop/src/lib/brand.ts
rg -qF "function BrandMarkAsset" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markLight}" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markDark}" apps/desktop/src/components/brand.tsx
rg -qF "src={brandAssets.markWhite}" apps/desktop/src/components/brand.tsx
rg -qF "function OverlayMark" apps/desktop/src/components/overlay-leaf.tsx
rg -qF "<BrandMark className=\"overlay-mark\" />" apps/desktop/src/components/overlay-leaf.tsx
rg -qF "<BrandMark className=\"overlay-watermark\" />" apps/desktop/src/components/overlay-leaf.tsx
rg -qF "<BrandMark className=\"onb-logo-mark\" />" apps/desktop/src/screens/onboarding.tsx
rg -qF "<BrandMark className=\"onb-folder-mark\" />" apps/desktop/src/screens/onboarding.tsx
rg -qF "<BrandMark className=\"onb-folder-mark\" />" apps/desktop/src/screens/home.tsx
! rg -qF "function BrandGlyph" apps/desktop/src
! rg -qF "viewBox=\"0 0 508 508\"" apps/desktop/src
! rg -qF "<rect width=\"211\"" apps/desktop/src
rg -qF "mask=\"url(#cerul-app-icon-cutout)\"" apps/desktop/public/brand/svg/cerul-menubar-template.svg
! rg -qF "translate(34 34)" apps/desktop/public/brand/svg/cerul-menubar-template.svg
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
rg -qF "trayImage.setTemplateImage(true)" apps/electron-shell/src/main.ts
rg -qF "\"background\": \"../desktop/public/brand/dmg/dmg-background.png\"" apps/electron-shell/package.json
rg -qF "\"oneClick\": false" apps/electron-shell/package.json
rg -qF "\"installerSidebar\": \"../desktop/public/brand/nsis/installerSidebar.bmp\"" apps/electron-shell/package.json
rg -qF "\"installerHeader\": \"../desktop/public/brand/nsis/installerHeader.bmp\"" apps/electron-shell/package.json
rg -qF "\"schemes\": [" apps/electron-shell/package.json
rg -qF "\"cerul-app\"" apps/electron-shell/package.json
rg -qF "\"from\": \"bin\"" apps/electron-shell/package.json
rg -qF "scripts/smoke-brand-assets-ui.sh" scripts/smoke.sh

echo "brand_assets_ui_smoke public_assets=present themed_logo=asset_backed themed_mark=asset_backed wordmark_lockups=app_icon legacy_inline_mark=absent menubar_template=app_icon_cutout electron_icon=present tray_icon=mac_template_nonmac_color bundle_identifier=desktop macos_min=10.15 protocols=registered resources=declared"
