#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [youtubeUnlimited, setYoutubeUnlimited]" apps/desktop/src
rg -qF "max_videos: youtubeUnlimited ? 0 : youtubeMax" apps/desktop/src
rg -qF "unlimited={youtubeUnlimited}" apps/desktop/src
rg -qF "setUnlimited={setYoutubeUnlimited}" apps/desktop/src
rg -qF '"addSource.youtube.validDetailAll": "All videos will be kept on disk."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.youtube.keepAll": "Keep all videos"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.folder.helper": "Cerul watches .mp4, .mkv, .webm, and .mov files inside this folder."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.youtube.helper": "Cerul will check this channel every 6 hours."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "api.previewRssSource" apps/desktop/src
rg -qF "preview.episode_count" apps/desktop/src
rg -qF "imageUrl={preview?.image_url ?? null}" apps/desktop/src
rg -qF "className=\"preview-image thumb\"" apps/desktop/src/components/source-preview.tsx
rg -qF ".preview-image.thumb" apps/desktop/src/styles/extensions.css
rg -qF ".preview-row" apps/desktop/src/styles/extensions.css
rg -qF ".type-card" apps/desktop/src/styles/extensions.css
rg -qF "previewRssSource" apps/desktop/src/lib/api.ts
rg -qF "/sources/preview/rss" apps/desktop/src/lib/api.ts
rg -qF "preview_feed" crates/cerul-sources/src/rss_podcast.rs
rg -qF "RssPodcastPreview" crates/cerul-sources/src/rss_podcast.rs
rg -qF "preview_rss_source" crates/cerul-api/src/lib.rs
rg -qF "\"/sources/preview/rss\"" crates/cerul-api/src/lib.rs
rg -qF "disabled={unlimited}" apps/desktop/src
rg -qF "setMax(Math.max(1, Number(event.currentTarget.value) || 1))" apps/desktop/src
rg -qF ".inline-toggle" apps/desktop/src/styles/extensions.css
rg -qF "scripts/smoke-add-source-ui.sh" scripts/smoke.sh
rg -qF "max_videos: Option<usize>" crates/cerul-sources/src/youtube.rs
rg -qF "max_videos == 0" crates/cerul-sources/src/youtube.rs
rg -qF "command.arg(\"--playlist-end\").arg(max_videos.to_string())" crates/cerul-sources/src/youtube.rs
rg -qF "fn zero_max_videos_means_unlimited" crates/cerul-sources/src/youtube.rs

echo "add_source_ui_smoke youtube_unlimited_toggle=enabled max_videos_zero_unlimited=enabled rss_preview=title_image_episode_count max_input_clamp=enabled brief_helper_copy=enabled"
