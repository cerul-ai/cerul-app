#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const defaultHotkeyLabel = \"Alt Space\"" apps/desktop/src/OverlayApp.tsx
rg -qF "const [hotkeyLabel, setHotkeyLabel]" apps/desktop/src/OverlayApp.tsx
rg -qF "formatHotkeyLabel(settings.global_hotkey)" apps/desktop/src/OverlayApp.tsx
rg -qF "const [sources, setSources]" apps/desktop/src/OverlayApp.tsx
rg -qF "api.listSources()" apps/desktop/src/OverlayApp.tsx
rg -qF "mapOverlayResult(record, items, sources, t)" apps/desktop/src/OverlayApp.tsx
rg -qF "function overlaySourceLabel" apps/desktop/src/OverlayApp.tsx
rg -qF "function overlaySourceName" apps/desktop/src/OverlayApp.tsx
rg -qF "source.type === \"web_video\" ? t(\"overlay.source.webVideo\") : t(\"overlay.source.youtube\")" apps/desktop/src/OverlayApp.tsx
rg -qF "compactUrlLabel(url, label)" apps/desktop/src/OverlayApp.tsx
rg -qF "compactUrlLabel(feedUrl, t(\"overlay.source.podcast\"))" apps/desktop/src/OverlayApp.tsx
rg -qF "function OverlayThumbGlyph" apps/desktop/src/OverlayApp.tsx
rg -qF "function OverlayHint" apps/desktop/src/OverlayApp.tsx
rg -qF "chunkType: record.chunk_type" apps/desktop/src/OverlayApp.tsx
rg -qF "sourceType: source?.type ?? null" apps/desktop/src/OverlayApp.tsx
rg -qF "function isVisualChunk" apps/desktop/src/OverlayApp.tsx
rg -qF "chunkType === \"understanding\"" apps/desktop/src/OverlayApp.tsx
rg -qF "contentType: item?.content_type ?? \"video\"" apps/desktop/src/OverlayApp.tsx
rg -qF "sourceType === \"rss_podcast\"" apps/desktop/src/OverlayApp.tsx
rg -qF "searchState === \"ready\"" apps/desktop/src/OverlayApp.tsx
rg -qF "<kbd>{hotkeyLabel}</kbd>" apps/desktop/src/OverlayApp.tsx
rg -qF "overlay-panel-body" apps/desktop/src/OverlayApp.tsx
rg -qF "overlay.searchPlaceholder" apps/desktop/src/OverlayApp.tsx
rg -qF "const overlayRetainQueryMs = 30_000" apps/desktop/src/OverlayApp.tsx
rg -qF "retainedQueryTimerRef" apps/desktop/src/OverlayApp.tsx
rg -qF "window.addEventListener(\"focus\", clearRetainedQueryTimer)" apps/desktop/src/OverlayApp.tsx
rg -qF "scheduleRetainedQueryReset()" apps/desktop/src/OverlayApp.tsx
rg -qF "void hideOverlay(true)" apps/desktop/src/OverlayApp.tsx
rg -qF "onMouseDown={handleBackdropMouseDown}" apps/desktop/src/OverlayApp.tsx
rg -qF "event.target === event.currentTarget" apps/desktop/src/OverlayApp.tsx
rg -qF "navigator.clipboard?.writeText" apps/desktop/src/OverlayApp.tsx
rg -qF "overlay.empty.noMatchesBody" apps/desktop/src/OverlayApp.tsx
rg -q "Global search permission" apps/desktop/src
rg -qF "settingsSection: params.get(\"section\")" apps/desktop/src
rg -qF "search.set(\"section\", params.settingsSection)" apps/desktop/src
rg -qF "invokeHostCommand(\"open_main_result\"" apps/desktop/src/OverlayApp.tsx
rg -qF "invokeHostCommand(\"open_accessibility_settings\"" apps/desktop/src
rg -qF "case \"open_accessibility_settings\"" apps/electron-shell/src/main.ts
rg -qF "case \"set_global_hotkey\"" apps/electron-shell/src/main.ts
rg -qF "function registerGlobalHotkey" apps/electron-shell/src/main.ts
rg -qF "process.env.CERUL_GLOBAL_HOTKEY" apps/electron-shell/src/main.ts
rg -qF "settingString(await readApiSettings(), \"global_hotkey\", defaultHotkey)" apps/electron-shell/src/main.ts
rg -qF "registerGlobalHotkey(await initialGlobalHotkey(), { throwOnFailure: false })" apps/electron-shell/src/main.ts
rg -qF "function showOverlay" apps/electron-shell/src/main.ts
rg -qF "setBounds({" apps/electron-shell/src/main.ts
rg -qF "height * 0.16" apps/electron-shell/src/main.ts
rg -qF "resize_overlay" apps/electron-shell/src/main.ts
rg -qF "case \"open_main_settings\"" apps/electron-shell/src/main.ts
rg -qF "case \"open_main_result\"" apps/electron-shell/src/main.ts
rg -qF "window.location.hash" apps/electron-shell/src/main.ts
rg -qF "settings?section=" apps/electron-shell/src/main.ts

echo "hotkey_overlay_smoke default_hotkey=Alt+Space configurable_settings=enabled env_smoke_override=enabled overlay_kbd=dynamic overlay_position=primary_top_third modality_icons=video_audio_image source_labels=readable future_proof_placeholder=enabled enter_command=open_main_result empty_state=local_library copy_link=clipboard backdrop_dismiss=enabled query_retention=30s accessibility_prompt=onboarding"
