import { describe, expect, it } from "vitest";

import {
  addSourceDisabled,
  classifyWebVideoUrl,
  uniqueYoutubeChannels,
  validateHttpUrl,
  youtubeChannelFromUrl,
} from "./validation";
import type { TFunction } from "./i18n";

const t: TFunction = (key, vars) => (vars ? `${key}:${JSON.stringify(vars)}` : key);

describe("validation helpers", () => {
  it("validates http urls and allowed hosts", () => {
    expect(validateHttpUrl("ftp://example.com", t)).toEqual({
      ok: false,
      message: "validation.url.protocol",
    });
    expect(validateHttpUrl("https://docs.example.com/path", t, ["example.com"])).toEqual({
      ok: true,
      hostname: "docs.example.com",
    });
    expect(validateHttpUrl("https://evil.test", t, ["example.com"])).toEqual({
      ok: false,
      message: 'validation.url.host:{"hosts":"example.com"}',
    });
  });

  it("classifies supported YouTube and Bilibili URLs", () => {
    expect(classifyWebVideoUrl("https://youtu.be/abc123", t)).toMatchObject({
      ok: true,
      platform: "youtube",
      sourceKind: "single",
    });
    expect(classifyWebVideoUrl("https://www.youtube.com/@cerul", t)).toMatchObject({
      ok: true,
      platform: "youtube",
      sourceKind: "author",
      url: "https://www.youtube.com/@cerul/videos",
    });
    expect(classifyWebVideoUrl("https://space.bilibili.com/42", t)).toMatchObject({
      ok: true,
      platform: "bilibili",
      sourceKind: "author",
      url: "https://space.bilibili.com/42/video",
    });
  });

  it("rejects unsupported web video URL shapes", () => {
    expect(classifyWebVideoUrl("https://www.youtube.com/playlist?list=abc", t)).toEqual({
      ok: false,
      message: "addSource.webVideo.playlistUnsupported",
    });
    expect(classifyWebVideoUrl("https://example.com/video", t)).toMatchObject({
      ok: false,
    });
  });

  it("normalizes onboarding channels and removes duplicates", () => {
    expect(youtubeChannelFromUrl("https://www.youtube.com/@cerul/videos", t)).toEqual({
      url: "https://www.youtube.com/@cerul/videos",
      name: "@cerul",
      subscribers: "validation.subscribersSync",
    });
    expect(
      uniqueYoutubeChannels([
        { url: "https://youtube.com/@cerul", name: "@cerul", subscribers: "1K" },
        { url: "https://youtube.com/@cerul", name: "@cerul duplicate", subscribers: "2K" },
        { url: " ", name: "blank", subscribers: "0" },
      ]),
    ).toEqual([{ url: "https://youtube.com/@cerul", name: "@cerul", subscribers: "1K" }]);
  });

  it("disables add-source submit until the active tab is valid", () => {
    expect(addSourceDisabled("folder", "", [], "", "", { status: "idle", message: null }, { status: "idle", message: null })).toBe(true);
    expect(addSourceDisabled("folder", "/tmp/videos", [], "", "", { status: "idle", message: null }, { status: "idle", message: null })).toBe(false);
    expect(addSourceDisabled("file", "", [], "", "", { status: "idle", message: null }, { status: "idle", message: null })).toBe(true);
    expect(addSourceDisabled("youtube", "", [], "https://youtu.be/x", "", { status: "validating", message: null }, { status: "idle", message: null })).toBe(true);
    expect(addSourceDisabled("youtube", "", [], "https://youtu.be/x", "", { status: "valid", message: null }, { status: "idle", message: null })).toBe(false);
    expect(addSourceDisabled("podcast", "", [], "", "https://example.com/feed.xml", { status: "idle", message: null }, { status: "error", message: "bad" })).toBe(true);
  });
});
