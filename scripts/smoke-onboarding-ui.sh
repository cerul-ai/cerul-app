#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "openDialog({ directory: true, multiple: true })" apps/desktop/src
rg -qF "api.addSource(\"folder_video\", { path: folder })" apps/desktop/src
rg -qF "api.addSource(\"youtube\", { url: channel.url, max_videos: 50 })" apps/desktop/src
rg -qF "folders={onboardingFolders}" apps/desktop/src
rg -qF "youtubeChannels={onboardingYoutubeChannels}" apps/desktop/src
rg -qF 'aria-label={t("onboarding.folder.chipsAria")}' apps/desktop/src
rg -qF 'aria-label={t("onboarding.youtube.listAria")}' apps/desktop/src
rg -qF '"onboarding.folder.chipsAria": "Selected folders"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"onboarding.youtube.listAria": "Selected YouTube channels"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF 'validDetail={t("onboarding.youtube.previewValidDetail")}' apps/desktop/src
rg -qF 'selectedSourceCount === 1' apps/desktop/src
rg -qF 'onboarding.final.addingOne' apps/desktop/src
rg -qF 'onboarding.final.addingOther' apps/desktop/src
rg -qF 'asr_model: "whisper-1"' apps/desktop/src
rg -qF 'active_embedding_profile: "gemini-embedding-2-3072"' apps/desktop/src
rg -qF '"onboarding.step0.kicker": "Cerul can index with remote APIs or a local model runtime."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"onboarding.model.asrDesc": "OpenAI whisper-1 by default, with GPT-4o transcribe options available."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"onboarding.model.embeddingDesc": "Gemini Embedding 2 creates one 3072-dimensional profile for text, images, audio, video, and documents."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"onboarding.model.connectionsDesc": "Add OpenAI and Gemini keys in Settings, or switch Models to local mode when the MLX runtime is ready."' apps/desktop/src/lib/i18n-catalog.ts
rg -qF ".onboarding-picker" apps/desktop/src/styles/extensions.css
rg -qF ".youtube-channel-card" apps/desktop/src/styles/extensions.css

echo "onboarding_ui_smoke folder_picker=multiple selected_folder_chips=removable youtube_validation=preview selected_youtube_channels=removable asr_model=whisper_1 embedding_profile=gemini_embedding_2 start_indexing_adds_sources=true"
