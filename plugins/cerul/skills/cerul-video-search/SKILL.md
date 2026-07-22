---
name: cerul-video-search
description: >-
  Search the user's local video, podcast, and talk library through the Cerul
  desktop app and cite results with exact timestamps. Use when the user asks
  what was said, shown, or presented in a video, podcast, talk, or meeting;
  when an answer should be backed by a quote from their media library; or when
  they want to find a moment or clip in their videos.
  当需要查证视频/播客/演讲里说过什么、或需要带时间戳的可引用出处时使用。
---

<!-- Static plugin copy. The desktop app installs a port-specific version via
     Settings → Connect Agent; source of truth for the generated content is
     crates/cerul-api/src/routes/v1/skill.rs. -->

# Cerul local video search

Cerul is a local-first video search app running on this machine. It exposes an
HTTP API at `http://127.0.0.1:23785/v1` (default port; loopback only, no auth
needed from this machine). Your library stays on this machine. Depending on
the processing mode selected in Cerul, a search query may be sent to the
configured remote model provider.

## Quick check

`GET http://127.0.0.1:23785/v1/status` → `library.indexed_items` says whether
there is content to search. If the request fails with connection refused, the
Cerul app is not running: ask the user to open Cerul, then retry. If the user
changed the API port in Cerul's settings, ask them for the port and substitute
it in every URL below.

## Search (primary tool)

```
POST http://127.0.0.1:23785/v1/search
Content-Type: application/json

{"query": "<natural language, zh or en>", "max_results": 10}
```

Each result in `results[]` contains:

- `type` — `transcript` | `visual` | `summary` | `document`
- `item.title`, `item.duration_sec`
- `time.timestamp` — `H:MM:SS` position of the match
- `text.quote` — verbatim quote to cite; `text.snippet` — wider context
- `evidence.clip.url` — playable video clip URL (local)
- `evidence.open_in_cerul` — deep link that opens Cerul at that exact second
- `score.match` — 0..1 relevance

Optional request field `ranking_preference`: `smart` (default) | `video` |
`image` | `document` | `audio`.

## Ask (extractive question answering)

```
POST http://127.0.0.1:23785/v1/ask
{"question": "<question>", "max_results": 6}
```

Returns `answer` (extractive) plus `citations[]` in the same shape as search
results. Prefer `/search` when the user wants moments; `/ask` when they want a
short answer with sources.

## Citing results

Always cite `item.title` + `time.timestamp`, and include the
`evidence.open_in_cerul` link so the user can jump to the exact second. Only
cite timestamps and quotes that a result actually returned — never invent them.

## Browse

- `GET http://127.0.0.1:23785/v1/items` — list library items
- `GET http://127.0.0.1:23785/v1/items/{id}/chunks` — transcript/visual chunks

Full reference: `GET http://127.0.0.1:23785/v1/openapi.json`.
