# Cerul Agent API v1 Implementation Plan

This document is the source of truth for moving Cerul's local and remote agent-facing API to a stable `/v1` contract. Each item should be implemented in order. After finishing an item, update its status here, commit the code and this document, push the branch, open or update the PR, and request `@codex review`.

## Goals

- Expose a stable agent-friendly `/v1` API for local Cerul Core and future official cloud API.
- Remove root-level public API routes such as `/search`, `/ask`, `/items`, and `/chunks/...`; migrate the desktop product to the replacement routes instead of preserving legacy aliases.
- Keep desktop functionality intact while moving product code to `/v1` or `/internal`.
- Let users configure the local Core port from Settings.
- Make returned search and ask payloads easy for both humans and agents to use, including playable local evidence URLs that can later become cloud URLs without breaking the schema.

## Route Policy

- `/v1/*`: stable read-oriented agent contract. This is the only route namespace agents should call.
- `/internal/*`: desktop-only app control surface for settings, providers, models, jobs, diagnostics, mutations, indexing, sources, playback state, and other product internals.
- Root-level routes are not retained as compatibility aliases. Removing them is intentional.
- Binary evidence routes may live under `/v1` when they are returned by `/v1/search`, `/v1/ask`, or `/v1/items/:id/chunks`.

## Target Public `/v1` Surface

- `GET /v1/status`
- `GET /v1/openapi.json`
- `POST /v1/search`
- `POST /v1/ask`
- `GET /v1/items`
- `GET /v1/items/:id`
- `GET /v1/items/:id/chunks`
- `GET /v1/chunks/:id/frame`
- `GET /v1/chunks/:id/video-segment`
- `GET /v1/chunks/:id/video-clip`

## Target Internal Surface

- `GET /internal/health`
- `GET /internal/metrics`
- `GET /internal/diagnostics`
- `GET /internal/diagnostics/indexing`
- `GET /internal/search/diagnostics`
- `POST /internal/search/rebuild`
- `GET/POST /internal/sources`
- `POST /internal/sources/preview/rss`
- `DELETE /internal/sources/:id`
- `POST /internal/sources/:id/pause`
- `POST /internal/sources/:id/resume`
- `POST /internal/sources/:id/retry-failed`
- `POST /internal/sources/:id/retry-discovery`
- `GET/POST /internal/moments`
- `DELETE /internal/moments/:id`
- `GET /internal/entities`
- `GET /internal/entities/:id`
- `GET /internal/weekly-review`
- `GET /internal/items`
- `GET/PATCH/DELETE /internal/items/:id`
- `GET/PATCH /internal/items/:id/playback`
- `POST /internal/items/:id/reindex`
- `GET /internal/items/:id/chunks`
- `GET/POST /internal/items/:id/understanding`
- `GET /internal/jobs`
- `POST /internal/jobs/:id/cancel`
- `GET /internal/usage/events`
- `GET /internal/usage/summary`
- `GET /internal/storage/usage`
- `GET /internal/models/catalog`
- `GET /internal/models/whisper`
- `POST /internal/models/whisper/:id/download`
- `GET /internal/models/whisper/auto-download-status`
- `GET /internal/models/embed/status`
- `POST /internal/models/embed/prepare`
- `GET /internal/models/local/capability`
- `POST /internal/models/local/prepare`
- `GET /internal/models/local/prepare-status`
- `POST /internal/models/local/prepare-cancel`
- `POST /internal/models/local/delete`
- `POST /internal/models/local/repair`
- `GET/POST/PATCH/DELETE /internal/providers/*`
- `GET/PATCH /internal/settings`

## Response Contract

All JSON `/v1` responses should include a `request_id`. Agent data routes should include:

- `execution`: where the work ran, for example `{ "target": "local", "privacy": "local_only" }`.
- `usage`: metering information when the route performs a counted operation. Local-only operations may be metered with `credits: 0`.
- Structured objects instead of UI-shaped flat fields.

`/v1/search` result shape:

- `id`: stable result/chunk id.
- `type`: `transcript`, `visual`, or future content class.
- `source`: `local_library` or future cloud source.
- `item`: `{ id, title, content_type, source_type, duration_sec }`.
- `time`: `{ start_sec, end_sec, timestamp }`.
- `text`: `{ snippet, quote }`.
- `evidence`: stable locator object with local playable URLs now and optional cloud locators later.
- `score`: `{ match, exact_match, similarity }`.

Evidence should include at least:

- `id`: stable evidence id derived from the chunk or future cloud evidence record.
- `kind`: `video_clip`, `frame`, or future evidence class.
- `clip`: current preferred clip locator, for example `{ "type": "local", "url": "http://127.0.0.1:<port>/v1/chunks/<id>/video-clip?..." }`.
- `preview`: current preferred frame locator.
- `open_in_cerul`: deep link back into the desktop app.
- Future additive `locators`: optional list with both local and cloud URLs after upload/share is available.

`/v1/ask` should state `mode`. Current implementation is extractive/template-based, not LLM RAG, so the initial mode should be `extractive`.

## Port Policy

- Default branded local port target: `23785`.
- Users can override the port in Settings.
- Config should persist in local settings as an integer port.
- The desktop shell and frontend must derive the API base URL from the configured port instead of hardcoding `7777`.
- Port changes should clearly state whether restart is required.
- `CERUL_API_PORT` may be supported as an environment override if it fits existing startup flow.

## Implementation Checklist

- [x] 1. Planning document added and maintained.
- [x] 2. Move desktop/internal backend routes from root to `/internal`, leaving no root compatibility routes.
- [x] 3. Migrate Electron shell, desktop frontend, menubar, CSP, tests, and helpers from root routes to `/internal` or `/v1` as appropriate.
- [x] 4. Add configurable API port in backend settings and `configured_addr`, with validation and tests.
- [x] 5. Add Settings UI controls and i18n copy for custom local Core port.
- [x] 6. Add `/v1/status` and `/v1/openapi.json`.
- [x] 7. Add `/v1/search` with agent-friendly request and response fields, including evidence URLs.
- [x] 8. Add `/v1/ask` with `mode`, citations, evidence, and usage metadata.
- [x] 9. Add `/v1/items`, `/v1/items/:id`, and `/v1/items/:id/chunks` agent responses.
- [x] 10. Add `/v1/chunks/:id/frame`, `/v1/chunks/:id/video-segment`, and `/v1/chunks/:id/video-clip`.
- [x] 11. Add or update tests proving root routes are gone and product routes still work.
- [x] 12. Update README, API examples, and smoke scripts to use `/v1`, `/internal`, and the configured port.
- [x] 13. Push each completed item, open/update the PR, and comment `@codex review`; 30-minute heartbeat monitoring was stopped per the 2026-06-26 follow-up instruction.

## Completion Log

- 2026-06-26: Added this implementation plan and marked item 1 complete.
- 2026-06-26: Moved the existing desktop API surface under `/internal`, removed root compatibility routes, migrated desktop/Electron/menubar calls, and added root-route removal coverage.
- 2026-06-26: Added configurable Core port support with default `23785`, settings validation, endpoint metadata, dynamic desktop API base URL wiring, and Settings UI controls.
- 2026-06-26: Added `/v1/status` and `/v1/openapi.json` with agent-facing status fields, v1-only OpenAPI paths, and route tests.
- 2026-06-26: Added `/v1/search` with structured agent results, local clip/preview evidence URL fields, usage metadata, and unsupported cloud-target validation.
- 2026-06-26: Added `/v1/ask` as extractive mode with shared v1 citation/evidence shape, local usage metadata, and mode validation.
- 2026-06-26: Added `/v1/items`, `/v1/items/:id`, and `/v1/items/:id/chunks` with stable item metadata, chunk context, pagination, local evidence locators, and privacy-preserving field selection.
- 2026-06-26: Added `/v1/chunks/:id/frame`, `/v1/chunks/:id/video-segment`, and `/v1/chunks/:id/video-clip` by exposing the existing local binary evidence handlers under the agent v1 namespace with route coverage.
- 2026-06-26: Expanded route regression coverage so removed root API routes return 404 and representative desktop `/internal` product routes remain available.
- 2026-06-26: Updated README examples and current smoke scripts to use `/v1`, `/internal`, default port `23785`, and configured-port helpers; added `v1_base_url` to local endpoint discovery metadata.
- 2026-06-26: Addressed review feedback by documenting the desktop item read routes and model setup mutation routes preserved under `/internal`.
- 2026-06-26: Addressed review feedback by aligning v1 deep links with the Electron router, checking local source files before returning clip evidence, translating public chunk type filters, preserving saved API ports in `run.sh`, and trimming request aliases before fallback.
- 2026-06-26: Addressed follow-up review feedback by exporting the resolved saved port for dev cleanup, defaulting `/v1/ask` to English unless Chinese is explicitly requested, omitting stale frame preview locators, and marking remote query embedding in v1 execution/usage metadata.
- 2026-06-26: Stopped the PR review heartbeat monitor per follow-up instruction while keeping the PR updated and requesting `@codex review` after the latest push.
