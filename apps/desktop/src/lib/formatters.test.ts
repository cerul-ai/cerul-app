import { describe, expect, it } from "vitest";

import {
  basenameFromPath,
  buildMomentCitation,
  cleanMediaTitle,
  compactPathDisplay,
  compactPathParent,
  errorMessage,
  extractChunkIdFromThumbnail,
  formatBytes,
  formatDuration,
  formatSpeed,
  formatTimestamp,
  formatUsd,
  parseTimestampSeconds,
  pluralize,
  sanitizeErrorText,
  uniqueStrings,
} from "./formatters";

describe("formatters", () => {
  it("formats durations and timestamps consistently", () => {
    expect(formatDuration(null)).toBe("Unknown");
    expect(formatDuration(0)).toBe("Unknown");
    expect(formatDuration(65)).toBe("1:05");
    expect(formatDuration(3661)).toBe("1:01:01");

    expect(formatTimestamp(null)).toBe("00:00");
    expect(formatTimestamp(-5)).toBe("00:00");
    expect(formatTimestamp(65)).toBe("1:05");
    expect(formatTimestamp(3661)).toBe("1:01:01");
    expect(parseTimestampSeconds("1:01:01")).toBe(3661);
    expect(parseTimestampSeconds("not-a-time")).toBe(0);
  });

  it("compacts and cleans local paths for display", () => {
    expect(basenameFromPath("/Users/me/Videos/demo.mp4")).toBe("demo.mp4");
    expect(basenameFromPath("C:\\Users\\me\\Videos\\demo.mp4")).toBe("demo.mp4");
    expect(compactPathParent("/Users/me/Videos/demo.mp4")).toBe("me/Videos");
    expect(compactPathDisplay("/Users/me/Videos/demo.mp4")).toBe("Videos/demo.mp4");
    expect(cleanMediaTitle("YTDown_YouTube_Product_Demo_Media_abc123_1080p.mp4")).toBe("Product Demo");
  });

  it("formats bytes, speeds, currency, and plurals", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatSpeed(null)).toBeNull();
    expect(formatSpeed(512 * 1024)).toBe("512 KB/s");
    expect(formatSpeed(2.25 * 1024 * 1024)).toBe("2.3 MB/s");
    expect(formatUsd(null)).toBe("$0.00");
    expect(formatUsd(0.005)).toBe("$0.0050");
    expect(formatUsd(1.2)).toBe("$1.20");
    expect(pluralize(1, "item")).toBe("1 item");
    expect(pluralize(2, "item")).toBe("2 items");
  });

  it("normalizes collections and safe error text", () => {
    expect(uniqueStrings([" alpha ", "", "beta", "alpha"])).toEqual(["alpha", "beta"]);
    expect(sanitizeErrorText("Missing key from .env")).toBe("Missing key");
    expect(errorMessage(new Error("Bad token (.env)"))).toBe("Bad token");
  });

  it("extracts chunk ids and builds citations", () => {
    expect(extractChunkIdFromThumbnail("/v1/chunks/chunk%201/frame")).toBe("chunk 1");
    expect(extractChunkIdFromThumbnail(null)).toBeNull();
    expect(
      buildMomentCitation({
        title: "Launch talk",
        timestamp: "1:23",
        quote: "pricing changes next quarter",
        link: "cerul-app://item/1",
      }),
    ).toBe("> pricing changes next quarter\n\n— Launch talk @ 1:23\ncerul-app://item/1");
  });
});
