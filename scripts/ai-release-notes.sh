#!/usr/bin/env bash
# Generate user-facing release notes for Cerul from git commits via an
# OpenAI-compatible LLM. Produces three channel variants: GitHub, Discord, Tweet.
#
# The LLM only summarizes the *changes*; links (app site + GitHub) are appended
# deterministically here so the URLs are always correct.
#
# Usage:
#   scripts/ai-release-notes.sh <tag> [--prev <tag>] [--out <dir>] \
#                               [--channels github,discord,tweet] [--print]
#
# Env (required): LLM_API_KEY, LLM_BASE_URL, LLM_MODEL
# Env (optional): DOWNLOAD_URL (default https://app.cerul.ai)
#                 REPO_URL     (default https://github.com/cerul-ai/cerul-app)
#
# Writes <out>/notes.github.md, <out>/notes.discord.md, <out>/notes.tweet.txt
set -euo pipefail

TAG="${1:-}"; shift || true
if [ -z "$TAG" ]; then
  echo "usage: $0 <tag> [--prev <tag>] [--out <dir>] [--channels github,discord,tweet] [--print]" >&2
  exit 2
fi

PREV=""
OUT="."
CHANNELS="github,discord,tweet"
PRINT=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prev) PREV="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    --channels) CHANNELS="$2"; shift 2;;
    --print) PRINT=1; shift;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

: "${LLM_API_KEY:?LLM_API_KEY is required}"
: "${LLM_BASE_URL:?LLM_BASE_URL is required}"
: "${LLM_MODEL:?LLM_MODEL is required}"
DOWNLOAD_URL="${DOWNLOAD_URL:-https://app.cerul.ai}"
REPO_URL="${REPO_URL:-https://github.com/cerul-ai/cerul-app}"

# --- collect commits in range -------------------------------------------------
if [ -z "$PREV" ]; then
  PREV="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "${TAG}^" 2>/dev/null || true)"
fi
if [ -n "$PREV" ]; then
  commits="$(git log --no-merges --pretty='- %s' "${PREV}..${TAG}")"
else
  commits="$(git log --no-merges --pretty='- %s' -n 40 "$TAG")"   # first release fallback
fi
# strip the one piece of pure noise every range carries; the model drops the rest
commits="$(printf '%s\n' "$commits" | grep -viE '^- (bump version to|merge )' || true)"
[ -n "$commits" ] || commits="- Maintenance and stability improvements"

CONTEXT="Cerul is a local-first desktop app that turns the video and audio you watch and hear -- talks, podcasts, lectures, recorded calls -- into a searchable second brain. It transcribes and indexes media locally, then lets users search by meaning across speech and on-screen text and jump to the exact moment."

USER_MSG="Release ${TAG}.

Commits since ${PREV:-start}:
${commits}"

# --- LLM call helper ----------------------------------------------------------
chat() {
  local sys="$1" req resp content
  req="$(jq -n --arg m "$LLM_MODEL" --arg s "$sys" --arg u "$USER_MSG" \
    '{model:$m, temperature:0.4, messages:[{role:"system",content:$s},{role:"user",content:$u}]}')"
  resp="$(curl -fsS --max-time 120 "$LLM_BASE_URL/chat/completions" \
    -H "Authorization: Bearer $LLM_API_KEY" -H "Content-Type: application/json" -d "$req")" \
    || { echo "LLM request failed for $TAG" >&2; return 1; }
  content="$(printf '%s' "$resp" | jq -r '.choices[0].message.content // empty')"
  if [ -z "$content" ]; then
    echo "LLM returned no content for $TAG. Raw response (head):" >&2
    printf '%s\n' "$resp" | head -c 800 >&2; echo >&2
    return 1
  fi
  printf '%s' "$content"
}

mkdir -p "$OUT"

# GitHub release notes: factual changelog + app download and GitHub links
# (kept uniform with the Discord and Tweet footers).
gen_github() {
  local body
  body="$(chat "You are a release-notes writer for Cerul. ${CONTEXT}
Convert raw git commit subjects into clean, user-facing GitHub release notes.
Group changes under '### New', '### Improved', and '### Fixed'; omit any group that would be empty.
One short bullet per real, user-noticeable change, in plain language. Only describe changes clearly present in the commit list; never invent features or filler.
Drop internal noise (version bumps, CI, build, tests, refactors, merges); never print commit hashes.
If nothing is user-facing, output exactly: Maintenance and stability improvements.
Output GitHub-flavored Markdown only -- no title, no preamble, no footer, no links, no sign-off.")"
  {
    printf '%s\n' "$body"
    printf '\n---\n📥 **Download:** %s · ⭐ **GitHub:** %s\n' "$DOWNLOAD_URL" "$REPO_URL"
  } > "$OUT/notes.github.md"
}

# Discord announcement: warm tone + both links (app site + GitHub).
gen_discord() {
  local body
  body="$(chat "You write the Discord release announcement for Cerul. ${CONTEXT}
Tone: warm, concise, community-facing.
Structure: (1) one short, friendly hook sentence about the main highlight of this release -- do NOT merely announce the version number (the embed title already shows it); (2) one bullet per real change, each starting with a single tasteful emoji, in plain language -- do NOT pad; a small release may have a single bullet.
Only describe changes clearly present in the commit list; never invent features, benefits, or filler.
Under 800 characters. Markdown. No title header, no @everyone, no hashtags, no links, no download line, no preamble or sign-off (links are appended separately).")"
  {
    printf '%s\n' "$body"
    printf '\n📥 Download: %s\n⭐ GitHub: %s\n' "$DOWNLOAD_URL" "$REPO_URL"
  } > "$OUT/notes.discord.md"
}

# Tweet/X draft: punchy body (no URLs) + both links appended.
gen_tweet() {
  local body
  body="$(chat "You write ONE X/Twitter post announcing Cerul ${TAG}. ${CONTEXT}
Lead with the single most exciting real user-facing change; at most one emoji; no hashtags; do NOT include any URL or link.
Only reference real changes from the commits; never invent.
Max 180 characters. Output plain text only: just the post body, no quotes, no preamble (links are appended separately).")"
  {
    printf '%s\n\n📥 %s\n⭐ %s\n' "$body" "$DOWNLOAD_URL" "$REPO_URL"
  } > "$OUT/notes.tweet.txt"
}

IFS=',' read -ra chs <<< "$CHANNELS"
for ch in "${chs[@]}"; do
  case "$ch" in
    github) gen_github;;
    discord) gen_discord;;
    tweet) gen_tweet;;
    *) echo "unknown channel: $ch" >&2; exit 2;;
  esac
done

if [ "$PRINT" = "1" ]; then
  for ch in "${chs[@]}"; do
    case "$ch" in
      github) printf '\n===== GitHub release notes (%s) =====\n' "$TAG"; cat "$OUT/notes.github.md";;
      discord) printf '\n===== Discord announcement (%s) =====\n' "$TAG"; cat "$OUT/notes.discord.md";;
      tweet) printf '\n===== Tweet draft (%s) =====\n' "$TAG"; cat "$OUT/notes.tweet.txt"; echo;;
    esac
  done
fi
