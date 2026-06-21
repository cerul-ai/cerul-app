// Search result mappers + helpers. Extracted from App.tsx (B13 Phase E).

import * as api from "./api";
import { formatTimestamp, parseTimestampSeconds } from "./formatters";
import type { TFunction } from "./i18n";
import type {
  Item,
  Result,
  ResultConfidence,
  ResultMatch,
  TranscriptLine,
} from "./types";

const RESULT_GROUP_WINDOW_SEC = 12;

export function mapResultMatch(
  record: api.SearchResultRecord,
  bestScore: number,
  t: TFunction,
): ResultMatch {
  const matchScore = resultMatchScore(record, bestScore);
  const confidence = resultConfidence(matchScore, 1);
  const scoreInfo = resultScoreInfo(record, bestScore, t);
  return {
    id: record.chunk_id,
    timestamp: formatTimestamp(record.start_sec),
    snippet: displaySnippet(record, t),
    chunkType: record.chunk_type,
    confidence,
    confidenceLabel: resultConfidenceLabel(confidence, t),
    scoreLabel: scoreInfo.label,
    scoreTitle: scoreInfo.title,
  };
}

export function mapSearchResult(
  record: api.SearchResultRecord,
  allItems: Item[],
  bestScore: number,
  t: TFunction,
): Result {
  const item = allItems.find((candidate) => candidate.id === record.item_id);
  const matchScore = resultMatchScore(record, bestScore);
  const confidence = resultConfidence(matchScore, 1);
  const scoreInfo = resultScoreInfo(record, bestScore, t);
  return {
    id: record.chunk_id,
    itemId: record.item_id,
    title: item?.title ?? record.item_id,
    source: item?.source ?? t("result.sourceFallback"),
    timestamp: formatTimestamp(record.start_sec),
    indexedAtEpoch: item?.indexedAtEpoch ?? null,
    duration: item?.duration ?? "",
    snippet: displaySnippet(record, t),
    color: item?.color ?? "steel",
    thumbnailUrl: resultThumbnailUrl(record, item),
    confidence,
    confidenceLabel: resultConfidenceLabel(confidence, t),
    score: matchScore,
    scoreLabel: scoreInfo.label,
    scoreTitle: scoreInfo.title,
    chunkType: record.chunk_type,
    moreMatches: [],
  };
}

export function mapSearchResults(
  records: api.SearchResultRecord[],
  allItems: Item[],
  t: TFunction,
): Result[] {
  const grouped = new Map<string, Result>();
  const bestScore = records
    .map((record) => record.score)
    .filter((score) => Number.isFinite(score) && score > 0)
    .sort((left, right) => right - left)[0] ?? 0;

  for (const record of records) {
    const groupKey = resultGroupKey(record);
    const existing = grouped.get(groupKey);
    if (existing) {
      existing.moreMatches.push(mapResultMatch(record, bestScore, t));
      continue;
    }

    grouped.set(groupKey, mapSearchResult(record, allItems, bestScore, t));
  }

  return Array.from(grouped.values());
}

export function resultConfidence(score: number, bestScore: number): ResultConfidence {
  if (!Number.isFinite(score) || !Number.isFinite(bestScore) || bestScore <= 0) {
    return "low";
  }
  const ratio = score / bestScore;
  if (ratio >= 0.82) {
    return "high";
  }
  if (ratio >= 0.48) {
    return "medium";
  }
  return "low";
}

export function resultConfidenceLabel(confidence: ResultConfidence, t: TFunction) {
  if (confidence === "high") {
    return t("result.confidence.strong");
  }
  if (confidence === "medium") {
    return t("result.confidence.partial");
  }
  return t("result.confidence.review");
}

export function resultScoreInfo(record: api.SearchResultRecord, bestScore: number, t: TFunction) {
  const matchScore = resultMatchScore(record, bestScore);
  const similarityScore = record.similarity_score;
  const similarityTitle =
    similarityScore !== null && Number.isFinite(similarityScore)
      ? `${t("result.score.similarityTitle")}: ${Math.round(Math.min(Math.max(similarityScore, 0), 1) * 100)}%`
      : null;
  const title = similarityTitle
    ? `${t("result.score.rankTitle")} · ${similarityTitle}`
    : t("result.score.rankTitle");
  if (!Number.isFinite(matchScore) || matchScore <= 0) {
    return {
      label: t("result.score.rank", { pct: 0 }),
      title,
    };
  }
  return {
    label: t("result.score.rank", {
      pct: Math.round(matchScore * 100),
    }),
    title,
  };
}

function resultMatchScore(record: api.SearchResultRecord, bestScore: number): number {
  if (
    record.match_score !== null &&
    record.match_score !== undefined &&
    Number.isFinite(record.match_score)
  ) {
    return Math.min(Math.max(record.match_score, 0), 1);
  }
  if (
    Number.isFinite(record.score) &&
    record.score > 0 &&
    Number.isFinite(bestScore) &&
    bestScore > 0
  ) {
    return Math.min(Math.max(record.score / bestScore, 0), 1);
  }
  return 0;
}

function resultThumbnailUrl(record: api.SearchResultRecord, item: Item | undefined): string | null {
  if (record.frame_path) {
    return api.chunkFrameUrl(record.chunk_id);
  }
  if (record.nearest_frame_chunk_id) {
    return api.chunkFrameUrl(record.nearest_frame_chunk_id);
  }
  return item?.thumbnailUrl ?? null;
}

function resultGroupKey(record: api.SearchResultRecord) {
  if (typeof record.start_sec === "number") {
    const bucket = Math.floor(record.start_sec / RESULT_GROUP_WINDOW_SEC);
    return `${record.item_id}:${bucket}`;
  }
  return `${record.item_id}:${record.chunk_type}`;
}

export function mapChunkRecords(records: api.ChunkRecord[]): TranscriptLine[] {
  const transcriptLines = records.filter((record) => record.chunk_type === "transcript_line");
  const spokenRecords = transcriptLines.length > 0
    ? transcriptLines
    : records.filter((record) => record.chunk_type === "transcript" || record.chunk_type === "audio");

  return spokenRecords.flatMap((record) => {
    if (!record.text) {
      return [];
    }
    return [
      {
        id: record.id,
        time: formatTimestamp(record.start_sec),
        text: record.text,
        startSec: record.start_sec,
        endSec: record.end_sec,
      },
    ];
  });
}

export function selectPlaybackChunkId(
  records: api.ChunkRecord[],
  timestamp: string,
  preferredChunkId?: string | null,
) {
  if (preferredChunkId && records.some((record) => record.id === preferredChunkId)) {
    return preferredChunkId;
  }

  const exact = records.find((record) => formatTimestamp(record.start_sec) === timestamp);
  if (exact) {
    return exact.id;
  }

  const targetSeconds = parseTimestampSeconds(timestamp);
  const timed = records
    .filter((record) => typeof record.start_sec === "number")
    .sort(
      (a, b) =>
        Math.abs((a.start_sec ?? 0) - targetSeconds) -
        Math.abs((b.start_sec ?? 0) - targetSeconds),
    );

  return timed[0]?.id ?? records[0]?.id ?? null;
}

export function resultModality(result: Result): import("./types").ResultModalityFilter {
  const chunkTypes = [
    result.chunkType,
    ...result.moreMatches.map((match) => match.chunkType ?? ""),
  ].filter(Boolean);
  const hasShown = chunkTypes.some(isShownChunkType);
  const hasSpoken = chunkTypes.some(isSpokenChunkType);
  const hasBoth = chunkTypes.some(isBothChunkType);

  if (hasBoth || (hasShown && hasSpoken)) {
    return "video";
  }
  if (hasShown) {
    return "image";
  }
  if (hasSpoken) {
    return "audio";
  }
  if (result.color === "amber") {
    return "audio";
  }
  if (result.color === "rose") {
    return "image";
  }
  return "video";
}

// The search backend (`cerul-search::fallback_snippet`) fills text-less visual /
// keyframe chunks with a hardcoded ENGLISH placeholder such as "Visual frame at
// 0:40". Left untouched it leaks English into a localized UI and repeats a
// timestamp the row already shows on its thumbnail and meta column. Reconstruct
// those exact placeholders so callers can recognise them and swap in a localized
// label instead. (We can't make the backend return an empty snippet — the Ask
// path filters empty snippets out of its answer context.)
export function backendFallbackSnippet(chunkType: string, startSec: number | null): string {
  let ts: string | null = null;
  if (startSec !== null) {
    const total = Math.max(0, Math.round(startSec));
    ts = `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
  }
  if (chunkType === "keyframe" || chunkType === "image" || chunkType === "ocr") {
    return ts ? `Visual frame at ${ts}` : "Visual match";
  }
  if (chunkType === "understanding") {
    return ts ? `Video understanding at ${ts}` : "Video understanding match";
  }
  return ts ? `Search match at ${ts}` : "Search match";
}

export function isBackendFallbackSnippet(
  snippet: string,
  chunkType: string,
  startSec: number | null,
): boolean {
  return snippet.trim() === backendFallbackSnippet(chunkType, startSec);
}

function displaySnippet(record: api.SearchResultRecord, t: TFunction) {
  const snippet = record.snippet.trim();
  if (
    snippet &&
    !looksLikeLocalPath(snippet) &&
    !isBackendFallbackSnippet(snippet, record.chunk_type, record.start_sec)
  ) {
    return snippet;
  }
  const timestamp = formatTimestamp(record.start_sec);
  if (isShownChunkType(record.chunk_type)) {
    return record.start_sec === null
      ? t("results.snippet.visualMatch")
      : t("results.snippet.visualFrameAt", { ts: timestamp });
  }
  if (isBothChunkType(record.chunk_type)) {
    return record.start_sec === null
      ? t("results.snippet.understandingMatch")
      : t("results.snippet.understandingAt", { ts: timestamp });
  }
  return record.start_sec === null
    ? t("results.snippet.searchMatch")
    : t("results.snippet.searchMatchAt", { ts: timestamp });
}

function looksLikeLocalPath(value: string) {
  return (
    /^\/Users\//.test(value) ||
    /^\/.+\/(cache|Library|Application Support)\//.test(value) ||
    /[\\/]cache[\\/].+\.(?:jpg|jpeg|png|webp|wav|mp4)$/i.test(value)
  );
}

function isSpokenChunkType(chunkType: string) {
  return chunkType === "transcript" || chunkType === "transcript_line" || chunkType === "audio";
}

function isShownChunkType(chunkType: string) {
  return chunkType === "keyframe" || chunkType === "image" || chunkType === "ocr";
}

function isBothChunkType(chunkType: string) {
  return chunkType === "understanding" || chunkType === "video";
}

export function resultMatchesTimeFilter(
  result: Result,
  filter: import("./types").ResultTimeFilter,
) {
  const seconds = parseTimestampSeconds(result.timestamp);
  if (filter === "first10") {
    return seconds < 10 * 60;
  }
  if (filter === "tenToThirty") {
    return seconds >= 10 * 60 && seconds < 30 * 60;
  }
  if (filter === "thirtyPlus") {
    return seconds >= 30 * 60;
  }
  return true;
}

export function resultMatchesConfidenceFilter(
  result: Result,
  filter: import("./types").ResultConfidenceFilter,
) {
  if (filter === "strong") {
    return result.confidence === "high" || result.confidence === "medium";
  }
  if (filter === "review") {
    return result.confidence === "low";
  }
  return true;
}
