<div align="center">
  <br />
  <a href="https://cerul.ai">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="apps/desktop/public/brand/icon-silver/cerul-icon-silver-256.png" />
      <img src="apps/desktop/public/brand/icon-graphite/cerul-icon-graphite-256.png" alt="Cerul" width="96" />
    </picture>
  </a>
  <h1>Cerul App</h1>
  <p><strong>A self-hosted second brain for everything you watch and hear.</strong></p>
  <p>Point it at your folders, YouTube channels, and podcast feeds. Cerul watches them locally, transcribes with provider keys you control or local models, and indexes the results on your machine — then lets you search by meaning across speech and on-screen content, and jump straight to the moment, from a desktop app, a global overlay, or the Cerul Core API.</p>

  <p>
    <a href="https://cerul.ai"><strong>Website</strong></a> &middot;
    <a href="https://github.com/cerul-ai/cerul"><strong>Main Repo</strong></a> &middot;
    <a href="https://x.com/cerul_hq"><img src="https://img.shields.io/badge/follow-%40cerul__hq-000?style=flat-square&logo=x" alt="Follow on X" /></a> &middot;
    <a href="https://discord.gg/qHDEMQB9vN"><img src="https://img.shields.io/badge/join-Discord-5865F2?style=flat-square&logo=discord&logoColor=white" alt="Join Discord" /></a>
  </p>

  <p>
    <a href="./LICENSE"><img alt="License" src="https://img.shields.io/badge/license-FSL--1.1--ALv2-3b82f6?style=flat-square" /></a>
    <img alt="Platforms" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-22c55e?style=flat-square" />
    <img alt="Status" src="https://img.shields.io/badge/status-v0.0.24-22c55e?style=flat-square" />
  </p>

  <p>
    <strong>English</strong> &middot;
    <a href="README.zh-CN.md">简体中文</a>
  </p>
</div>

<br />

> [!NOTE]
> **Initial release.** Cerul App is the source-available, self-hostable companion to [Cerul Cloud](https://cerul.ai). Current version: **0.0.24**. The core is functional — desktop shell, Cerul Core, indexing pipeline, hybrid search, overlay, and tray all run today. Public release builds are published through GitHub Releases; macOS artifacts are signed and notarized when the release workflow has Developer ID credentials.

## Why Cerul App

Most of what you learn lives in video and audio — talks, podcasts, lectures, recorded calls — and it's the hardest content to search. Transcripts capture what was *said*; the rest is locked inside files you'll never scrub through again.

Cerul App turns your own media into a searchable, **local-first** memory:

- **Your machine, your data.** Media, transcripts, and the vector index all stay on disk. Inference runs through provider keys *you* control, or fully local models — no Cerul account required.
- **Search by meaning.** Hybrid retrieval combines full-text (SQLite/FTS) with vector search (a bundled local [Qdrant](https://qdrant.tech)) so you find the moment, not just the keyword.
- **Always on, out of the way.** A global hotkey overlay, menu-bar tray, background indexing, and start-at-login keep it one keystroke away.
- **Agent-ready.** Cerul Core exposes a local REST API on `127.0.0.1:7777` so coding agents and scripts can query your library.

## How it works

The indexing pipeline is built for reliability — text search stays available even if embedding fails:

1. **Fetch** media from local folders, YouTube (`yt-dlp`), or podcast RSS.
2. **Extract** audio and sample frames with `ffmpeg`.
3. **Transcribe** via a Remote API provider or the local Qwen3-VL / MLX runtime.
4. **Index text** into SQLite/FTS immediately — searchable right away.
5. **Embed** transcript chunks and write vectors to Qdrant when embedding succeeds.

> Visual understanding (slides, charts, on-screen text via Gemini) is an opt-in beta enrichment on an item's detail page, not a required step in the pipeline.

## Sources & surfaces

| Sources | Surfaces | Inference |
|---|---|---|
| Local folders | Desktop window (library, sources, settings, detail) | **Remote API** — your provider keys (default) |
| YouTube channels & videos | Global search overlay (hotkey) | **Local model** — Qwen3-VL / MLX (macOS arm64) |
| Podcast RSS feeds | Local REST API (`127.0.0.1:7777`) | |

## Quickstart

> Requires Rust (stable), Node 22 + pnpm 9, and native build tools (`ffmpeg`, `protobuf`, `cmake`).

```bash
git clone https://github.com/cerul-ai/cerul-app.git
cd cerul-app
pnpm install

./run.sh
```

For a clean rebuild that clears build caches first:

```bash
./rebuild.sh
```

## Configuration

Configure provider connections in the app's **Settings → Models** screen. OpenAI-compatible endpoints work too: enter the API base URL, for example `https://api.lazu.ai/v1`, then use model discovery or type the model ID directly.

For source development, `run.sh` can also load a local `.env` file with default provider values. This is only a developer convenience:

```bash
# Transcription (ASR)
CERUL_ASR_MODEL=whisper-1
CERUL_ASR_API_KEY=...
CERUL_ASR_BASE_URL=https://api.openai.com/v1

# Embeddings
CERUL_EMBEDDING_MODEL=...
CERUL_EMBEDDING_API_KEY=...
CERUL_EMBEDDING_BASE_URL=...
```

You can also switch to a fully local model (Qwen3-VL / MLX) in the app's Models settings.

## Cerul Core API

Once the app is running, query your library over HTTP — handy for agents and automation:

```bash
# Health check
curl 127.0.0.1:7777/health

# Search by meaning
curl -X POST 127.0.0.1:7777/search \
  -H 'content-type: application/json' \
  -d '{"q": "what did they say about scaling laws"}'
```

Other routes cover sources (`/sources`), items (`/items`), and reindexing. The full contract is served live at `127.0.0.1:7777/openapi.json`.

## Project layout

```text
apps/
  desktop/         Frontend UI (library, sources, settings, overlay)
  electron-shell/  Electron runtime, tray, hotkeys, media streaming
crates/            Rust core — API, storage, indexing, search, sources
mlx-sidecar/       Local model runtime (Qwen3-VL / MLX, macOS arm64)
scripts/           Build, packaging, and smoke-test scripts
```

## Status & roadmap

Cerul App is in alpha. Current release: **0.0.24**. The foundation works end to end, and the release workflow now gates public macOS artifacts on signing, notarization, and installed-build smoke coverage.

**Working today**
- Electron desktop shell, local REST API, storage, and indexing pipeline
- Hybrid (FTS + vector) search, search overlay, tray, notifications, start-at-login
- Folder, YouTube, and RSS sources; Remote API and local-model inference

**Next release hardening**
- Windows/Linux packaging and signing
- Update metadata and auto-update rollout checks
- Broader installed-build release smoke coverage
- Third-party binary license review (`ffmpeg`, `yt-dlp`, `qdrant`)

Want newer ready-to-install builds? Star and watch the repo — public builds ship as GitHub Releases.

## How this fits with Cerul

Cerul App is the **source-available, self-hosted** layer of the [Cerul](https://github.com/cerul-ai/cerul) platform — run it on your own machine with your own keys. [Cerul Cloud](https://cerul.ai) is the hosted service for teams that want managed indexing, the video search API, and account-backed sync. The app works fully standalone; the Cloud account is optional. The Cerul Cloud account backend is not included in this repository; the desktop client only calls its public account API when you sign in.

## Project governance

- [`SECURITY.md`](SECURITY.md), [`PRIVACY.md`](PRIVACY.md), [`CONTRIBUTING.md`](CONTRIBUTING.md), [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md), and [`TRADEMARKS.md`](TRADEMARKS.md).

## Contributing

Issues and pull requests are welcome. For development, verify your change before opening a PR:

```bash
cargo check --workspace
pnpm --filter @cerul/desktop build
scripts/smoke.sh
```

## License

[FSL-1.1-ALv2](LICENSE) © Cerul. Source-available; each release converts to Apache-2.0 two years after it ships.
