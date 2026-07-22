//! Agent skill bundle: generates the `cerul-video-search` skill directory that
//! teaches a local agent (Claude Code, Codex CLI, …) how to call the local API.
//! Served as JSON (consumed by the desktop installer) and as a tar archive
//! (consumed by the copy-paste `curl | tar -x` install path).

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::{new_id, ApiResult, ApiState};

pub(crate) const SKILL_DIR_NAME: &str = "cerul-video-search";
// The generated skill ships with the desktop app, whose version is bumped by
// the release workflow. The Rust workspace version intentionally remains
// independent, so use the app manifest value injected by build.rs.
pub(crate) const SKILL_VERSION: &str = env!("CERUL_APP_VERSION");

#[derive(Serialize)]
pub(crate) struct SkillFile {
    pub path: &'static str,
    pub content: String,
}

#[derive(Serialize)]
pub(crate) struct V1AgentSkillResponse {
    pub request_id: String,
    pub version: &'static str,
    pub dir_name: &'static str,
    pub files: Vec<SkillFile>,
}

pub(crate) fn skill_files(base_url: &str) -> Vec<SkillFile> {
    vec![
        SkillFile {
            path: "SKILL.md",
            content: skill_md(base_url),
        },
        SkillFile {
            path: "references/api.md",
            content: api_reference_md(base_url),
        },
    ]
}

fn skill_md(base_url: &str) -> String {
    format!(
        r#"---
name: cerul-video-search
description: >-
  Search the user's local video, podcast, and talk library through the Cerul
  desktop app and cite results with exact timestamps. Use when the user asks
  what was said, shown, or presented in a video, podcast, talk, or meeting;
  when an answer should be backed by a quote from their media library; or when
  they want to find a moment or clip in their videos.
  当需要查证视频/播客/演讲里说过什么、或需要带时间戳的可引用出处时使用。
metadata:
  version: {version}
  base_url: {base_url}
---

# Cerul local video search

Cerul is a local-first video search app running on this machine. It exposes an
HTTP API at `{base_url}` (loopback only; no auth needed from this machine).
Your library stays on this machine. Depending on the processing mode selected
in Cerul, a search query may be sent to the configured remote model provider.

## Quick check

`GET {base_url}/status` → `library.indexed_items` says whether there is content
to search. If the request fails with connection refused, the Cerul app is not
running: ask the user to open Cerul, then retry.

## Search (primary tool)

```
POST {base_url}/search
Content-Type: application/json

{{"query": "<natural language, zh or en>", "max_results": 10}}
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
POST {base_url}/ask
{{"question": "<question>", "max_results": 6}}
```

Returns `answer` (extractive) plus `citations[]` in the same shape as search
results. Prefer `/search` when the user wants moments; `/ask` when they want a
short answer with sources.

## Citing results

Always cite `item.title` + `time.timestamp`, and include the
`evidence.open_in_cerul` link so the user can jump to the exact second. Only
cite timestamps and quotes that a result actually returned — never invent them.

## Browse

- `GET {base_url}/items` — list library items
- `GET {base_url}/items/{{id}}/chunks` — transcript/visual chunks of one item

Full parameter and response reference: `references/api.md` in this skill
folder, or `GET {base_url}/openapi.json`.
"#,
        version = SKILL_VERSION,
        base_url = base_url,
    )
}

fn api_reference_md(base_url: &str) -> String {
    format!(
        r#"# Cerul local API reference (agent surface)

Base URL: `{base_url}` · version {version} · loopback requests need no auth.
If Cerul binds to `0.0.0.0` (LAN mode), remote calls need
`Authorization: Bearer <remote_api_key>`.

## POST /search

| Field | Type | Notes |
| --- | --- | --- |
| `query` | string | Required. Natural language, Chinese or English. |
| `max_results` | int | 1–50, default 10. |
| `ranking_preference` | enum | `smart` (default), `video`, `image`, `document`, `audio`. |

Response: `{{ request_id, results: [Result], execution }}`.

`Result`:

```json
{{
  "id": "chk_…",
  "type": "transcript",
  "source": "local_library",
  "item": {{ "id": "itm_…", "title": "…", "content_type": "video", "source_type": "file", "duration_sec": 3120.0 }},
  "time": {{ "start_sec": 754.2, "end_sec": 762.8, "timestamp": "12:34" }},
  "text": {{ "snippet": "…", "quote": "…" }},
  "evidence": {{
    "id": "chk_…",
    "kind": "video_clip",
    "clip": {{ "type": "local", "url": "{base_url}/chunks/chk_…/video-clip?before_sec=3&after_sec=5" }},
    "preview": {{ "type": "local", "url": "{base_url}/chunks/chk_…/frame" }},
    "open_in_cerul": "cerul-app://item/itm_…?playbackChunkId=chk_…&t=754"
  }},
  "score": {{ "match": 0.91, "exact_match": false, "similarity": 0.83 }}
}}
```

## POST /ask

| Field | Type | Notes |
| --- | --- | --- |
| `question` | string | Required (aliases: `query`, `q`). |
| `max_results` | int | 1–8, default 6. |
| `locale` | string | Answer language hint, e.g. `zh-CN`. |

Response: `{{ request_id, mode: "extractive", answer, citations: [Result], execution }}`.

## GET /items

Query: `limit` (≤100, default 50), `cursor`, `status`, `source_id`,
`source_type`. Response: `{{ items: [...], page: {{ next_cursor }} }}`.

## GET /items/{{id}}/chunks

Query: `limit` (≤250, default 100), `cursor`, `from_sec`, `to_sec`,
`type` (`transcript` | `visual` | `summary`). Chunks carry text and timing for
one item.

## Evidence endpoints

- `GET /chunks/{{id}}/frame` — still frame (image)
- `GET /chunks/{{id}}/video-segment` — full source segment (supports Range)
- `GET /chunks/{{id}}/video-clip?before_sec=3&after_sec=5` — short clip around
  the match

## GET /status

`{{ status, version, library: {{ total_items, indexed_items, … }}, search: {{ retrieval_mode }} }}`
— `retrieval_mode` of `empty` means nothing is indexed yet.

## Errors

Non-2xx responses carry `{{ "error": {{ "code", "message" }} }}`. Connection
refused means the Cerul app is not running.
"#,
        version = SKILL_VERSION,
        base_url = base_url,
    )
}

fn skill_tar_bytes(base_url: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for file in skill_files(base_url) {
        let bytes = file.content.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                format!("{SKILL_DIR_NAME}/{}", file.path),
                bytes,
            )
            .expect("in-memory tar write cannot fail");
    }
    builder
        .into_inner()
        .expect("in-memory tar finish cannot fail")
}

pub(crate) async fn v1_agent_skill(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<Json<V1AgentSkillResponse>> {
    let base_url = super::v1_base_url(&headers, &state.paths);
    Ok(Json(V1AgentSkillResponse {
        request_id: new_id("req"),
        version: SKILL_VERSION,
        dir_name: SKILL_DIR_NAME,
        files: skill_files(&base_url),
    }))
}

pub(crate) async fn v1_agent_skill_tar(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let base_url = super::v1_base_url(&headers, &state.paths);
    let bytes = skill_tar_bytes(&base_url);
    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-tar"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"cerul-video-search-skill.tar\""),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://127.0.0.1:23785/v1";

    #[test]
    fn skill_md_embeds_name_version_and_base_url() {
        let files = skill_files(BASE);
        let skill = files
            .iter()
            .find(|file| file.path == "SKILL.md")
            .expect("SKILL.md present");
        assert!(skill.content.starts_with("---\n"));
        assert!(skill.content.contains("name: cerul-video-search"));
        assert!(skill.content.contains(&format!("version: {SKILL_VERSION}")));
        let app_manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../package.json"))
                .expect("root package.json parses");
        assert_eq!(Some(SKILL_VERSION), app_manifest["version"].as_str());
        assert!(skill.content.contains(BASE));
        assert!(skill.content.contains("a search query may be sent"));
        assert!(!skill.content.contains("nothing leaves the machine"));
        let reference = files
            .iter()
            .find(|file| file.path == "references/api.md")
            .expect("references/api.md present");
        assert!(reference.content.contains("POST /search"));
        assert!(reference.content.contains(BASE));
    }

    #[test]
    fn skill_tar_contains_both_entries() {
        let bytes = skill_tar_bytes(BASE);
        // tar archives are 512-byte aligned and end with two zero blocks.
        assert!(bytes.len() > 1024);
        assert_eq!(bytes.len() % 512, 0);
        let mut archive = tar::Archive::new(bytes.as_slice());
        let paths: Vec<String> = archive
            .entries()
            .expect("valid tar")
            .map(|entry| {
                entry
                    .expect("valid entry")
                    .path()
                    .expect("utf8 path")
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                format!("{SKILL_DIR_NAME}/SKILL.md"),
                format!("{SKILL_DIR_NAME}/references/api.md"),
            ]
        );
    }
}
