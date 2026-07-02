import { describe, expect, it } from "vitest";

import type * as api from "./api";
import {
  backendFallbackSnippet,
  isBackendFallbackSnippet,
  mapChunkRecords,
  mapSearchResults,
  resultConfidence,
  resultMatchesConfidenceFilter,
  resultMatchesTimeFilter,
  resultModality,
  selectPlaybackChunkId,
} from "./results";
import type { Item, Result } from "./types";
import type { TFunction } from "./i18n";

const t: TFunction = (key, vars) => {
  if (key === "result.score.rank") {
    return `rank ${vars?.pct}`;
  }
  if (key === "results.snippet.visualFrameAt") {
    return `visual ${vars?.ts}`;
  }
  if (key === "results.snippet.searchMatchAt") {
    return `search ${vars?.ts}`;
  }
  if (key === "results.snippet.documentMatch") {
    return "document match";
  }
  if (key === "result.timestamp.document") {
    return "Document";
  }
  if (key === "detail.document.locatorWithSection") {
    return `p. ${vars?.page} · ${vars?.section}`;
  }
  if (key === "detail.document.page") {
    return `p. ${vars?.page}`;
  }
  if (key === "detail.document.locatorFallback") {
    return "Document";
  }
  return key;
};

function record(overrides: Partial<api.SearchResultRecord>): api.SearchResultRecord {
  return {
    item_id: "item-1",
    chunk_type: "transcript",
    start_sec: 10,
    end_sec: 15,
    snippet: "pricing changed",
    frame_path: null,
    score: 0.8,
    similarity_score: 0.6,
    ...overrides,
  };
}

const item = {
  id: "item-1",
  title: "Launch",
  source: "YouTube",
  duration: "12:00",
  indexedAtEpoch: 1710000000,
  color: "steel",
  thumbnailUrl: "thumb.jpg",
} as Item;

function result(overrides: Partial<Result>): Result {
  return {
    itemId: "item-1",
    playbackChunkId: "chunk-1",
    startSec: 0,
    endSec: 1,
    title: "Launch",
    source: "YouTube",
    timestamp: "0:00",
    indexedAtEpoch: null,
    duration: "12:00",
    snippet: "snippet",
    color: "steel",
    thumbnailUrl: null,
    confidence: "medium",
    confidenceLabel: "partial",
    score: 0.5,
    rankScore: 0,
    scoreLabel: "rank 50",
    scoreTitle: "rank",
    chunkType: "transcript",
    moreMatches: [],
    ...overrides,
  };
}

describe("results helpers", () => {
  it("maps and groups backend search records into UI results", () => {
    const results = mapSearchResults(
      [
        record({ chunk_id: "chunk-a", score: 0.8, start_sec: 10 }),
        record({ chunk_id: "chunk-b", score: 0.4, start_sec: 11, snippet: "/Users/me/cache/frame.jpg", chunk_type: "keyframe" }),
      ],
      [item],
      t,
    );

    expect(results).toHaveLength(1);
    expect(results[0]).toMatchObject({
      itemId: "item-1",
      playbackChunkId: "chunk-a",
      title: "Launch",
      source: "YouTube",
      timestamp: "0:10",
      confidence: "high",
      scoreLabel: "rank 100",
    });
    expect(results[0].moreMatches).toHaveLength(1);
    expect(results[0].moreMatches[0]).toMatchObject({
      playbackChunkId: "chunk-b",
      snippet: "visual 0:11",
      confidence: "medium",
    });
  });

  it("handles fallback snippets and confidence buckets", () => {
    expect(backendFallbackSnippet("keyframe", 40)).toBe("Visual frame at 0:40");
    expect(isBackendFallbackSnippet("Visual frame at 0:40", "keyframe", 40)).toBe(true);
    expect(resultConfidence(0.9, 1)).toBe("high");
    expect(resultConfidence(0.5, 1)).toBe("medium");
    expect(resultConfidence(0.2, 1)).toBe("low");
    expect(resultConfidence(1, 0)).toBe("low");
  });

  it("maps chunk records and selects playback chunks", () => {
    const chunks: api.ChunkRecord[] = [
      { id: "audio-1", item_id: "item-1", chunk_type: "audio", start_sec: 1, end_sec: 2, text: "audio", frame_path: null, metadata: {} },
      { id: "line-1", item_id: "item-1", chunk_type: "transcript_line", start_sec: 12, end_sec: 13, text: "line", frame_path: null, metadata: {} },
      { id: "line-2", item_id: "item-1", chunk_type: "transcript_line", start_sec: 25, end_sec: 27, text: null, frame_path: null, metadata: {} },
    ];

    expect(mapChunkRecords(chunks)).toEqual([
      { id: "line-1", time: "0:12", text: "line", startSec: 12, endSec: 13 },
    ]);
    expect(selectPlaybackChunkId(chunks, "0:12")).toBe("line-1");
    expect(selectPlaybackChunkId(chunks, "0:26")).toBe("line-2");
    expect(selectPlaybackChunkId(chunks, "0:26", "audio-1")).toBe("audio-1");
  });

  it("maps document chunks as readable evidence rows", () => {
    const chunks: api.ChunkRecord[] = [
      {
        id: "doc-1",
        item_id: "item-1",
        chunk_type: "document",
        start_sec: null,
        end_sec: null,
        text: "The roadmap section mentions rollout risk.",
        frame_path: null,
        metadata: { page: 2, section: "Roadmap" },
      },
    ];

    expect(mapChunkRecords(chunks, t)).toEqual([
      {
        id: "doc-1",
        time: "p. 2 · Roadmap",
        text: "The roadmap section mentions rollout risk.",
        startSec: null,
        endSec: null,
      },
    ]);
  });

  it("filters results by modality, time, and confidence", () => {
    const audioResult = result({
      timestamp: "12:00",
      confidence: "high",
      color: "amber",
      chunkType: "audio",
    });
    const visualResult = result({
      timestamp: "35:00",
      confidence: "low",
      color: "rose",
      chunkType: "keyframe",
    });
    const documentResult = result({
      timestamp: "Document",
      confidence: "medium",
      chunkType: "document",
    });

    expect(resultModality(audioResult)).toBe("audio");
    expect(resultModality(visualResult)).toBe("image");
    expect(resultModality(documentResult)).toBe("document");
    expect(resultMatchesTimeFilter(audioResult, "tenToThirty")).toBe(true);
    expect(resultMatchesTimeFilter(visualResult, "thirtyPlus")).toBe(true);
    expect(resultMatchesConfidenceFilter(audioResult, "strong")).toBe(true);
    expect(resultMatchesConfidenceFilter(visualResult, "review")).toBe(true);
  });

  it("labels document search results without fake timestamps", () => {
    const results = mapSearchResults(
      [record({ chunk_id: "doc-1", chunk_type: "document", start_sec: null, snippet: "" })],
      [item],
      t,
    );

    expect(results[0]).toMatchObject({
      playbackChunkId: "doc-1",
      timestamp: "Document",
      snippet: "document match",
      chunkType: "document",
    });
  });
});
