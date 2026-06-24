#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [webVideoPreview, setWebVideoPreview]" apps/desktop/src
rg -qF "classifyWebVideoUrl(value, t)" apps/desktop/src
rg -qF "preview.sourceKind === \"author\"" apps/desktop/src
rg -qF "requestConfirm({" apps/desktop/src/dialogs/add-source-dialog.tsx
rg -qF "onAddSource(\"web_video\"" apps/desktop/src
rg -qF "platform: preview.platform" apps/desktop/src
rg -qF "source_kind: preview.sourceKind" apps/desktop/src
rg -qF "webVideoKeepAll" apps/desktop/src/dialogs/add-source-dialog.tsx
rg -qF "max_videos:" apps/desktop/src/dialogs/add-source-dialog.tsx
rg -qF "const DEFAULT_WEB_VIDEO_AUTHOR_MAX = 20" apps/desktop/src/dialogs/add-source-dialog.tsx
rg -qF "setWebVideoMax(clampWebVideoMax(value))" apps/desktop/src/dialogs/add-source-dialog.tsx
rg -qF '"addSource.folder.helper": "Cerul watches .mp4, .mkv, .webm, and .mov files inside this folder."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.youtube.helper": "Supports one YouTube/Bilibili video or an author homepage. Author homepages import the latest 20 videos by default; keep all only when explicitly selected."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.validDetailSingle": "This video will be downloaded locally before indexing."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.validDetailAuthor": "The latest {max} videos from this author homepage will be downloaded and indexed."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.authorMaxHint": "The latest {max} videos will be imported. Batch imports use the browser session."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.playlistUnsupported": "YouTube playlists are not supported yet. Use a single video or author homepage URL."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.confirmAuthor.title": "Download all videos from this author?"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"addSource.webVideo.confirmAuthorLimited.title": "Download the latest {max} videos from this author?"' apps/desktop/src/lib/i18n-catalog.ts
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
rg -qF "setMax(Math.max(1, Number(event.currentTarget.value) || 1))" apps/desktop/src
rg -qF "scripts/smoke-add-source-ui.sh" scripts/smoke.sh
rg -qF "pub mod web_video" crates/cerul-sources/src/lib.rs
rg -qF "\"web_video\" => Ok(Box::new(web_video::WebVideo::new(config)?))" crates/cerul-sources/src/lib.rs
rg -qF "WebVideoSourceKind::Author" crates/cerul-sources/src/web_video.rs
rg -qF "ytdlp_access_candidate_limit(self.max_videos)" crates/cerul-sources/src/web_video.rs
rg -qF "fn author_defaults_to_twenty_videos" crates/cerul-sources/src/web_video.rs

echo "add_source_ui_smoke web_video_single_and_author=enabled author_confirmation=enabled author_default_max_videos=20 rss_preview=title_image_episode_count max_input_clamp=enabled brief_helper_copy=enabled"
