import { describe, expect, it } from "vitest";
import { sourceConnectorDisplayName } from "./sources";
import type { Source } from "./types";

function source(name: string, type: Source["type"] = "youtube"): Source {
  return {
    id: name,
    type,
    name,
    status: "active",
    items: 0,
    failedItems: 0,
    lastPolled: "Never",
    lastPolledEpoch: null,
    error: null,
    fixSettingsSection: null,
  };
}

describe("sourceConnectorDisplayName", () => {
  it.each([
    ["youtube.com/shorts/short-123", "YouTube · short-123"],
    ["youtube.com/live/live-456", "YouTube · live-456"],
    ["youtube.com/c/CerulAI", "YouTube · CerulAI"],
    ["youtube.com/user/cerul-user", "YouTube · cerul-user"],
    ["youtube.com/channel/UC123_channel", "YouTube · UC123_channel"],
    ["youtube.com/@cerul", "YouTube · @cerul"],
  ])("keeps YouTube path identity for %s", (name, label) => {
    expect(sourceConnectorDisplayName(source(name), "YouTube")).toBe(label);
  });

  it("keeps short-link identity for Bilibili sources", () => {
    expect(sourceConnectorDisplayName(source("b23.tv/abc123", "web_video"), "Bilibili")).toBe(
      "Bilibili · abc123",
    );
  });

  it("keeps the feed path when podcast feeds share a host", () => {
    expect(
      sourceConnectorDisplayName(
        source("feeds.example.com/shows/cerul/feed.xml", "podcast"),
        "Podcast",
      ),
    ).toBe("feeds.example.com · shows/cerul/feed.xml");
  });

  it("falls back to the stored name when a URL has no explicit identity", () => {
    expect(sourceConnectorDisplayName(source("youtube.com"), "YouTube")).toBe("youtube.com");
  });
});
