import { describe, expect, it } from "vitest";

import type { AppData } from "./types";
import { searchIndexIsSettling } from "./search-index";

function appData(overrides: Partial<AppData> = {}) {
  return {
    sources: [],
    items: [],
    jobs: [],
    jobSummary: null,
    settings: {},
    whisperModels: [],
    daemonStatus: null,
    version: null,
    ...overrides,
  } as AppData;
}

describe("searchIndexIsSettling", () => {
  it("waits for hidden refreshes while indexing is active", () => {
    const data = appData({
      jobSummary: { search_refresh_jobs: 1 } as AppData["jobSummary"],
    });

    expect(searchIndexIsSettling(data)).toBe(true);
  });

  it("does not wait for queued hidden refreshes while indexing is paused", () => {
    const data = appData({
      settings: { indexing_paused: true },
      jobSummary: { search_refresh_jobs: 1 } as AppData["jobSummary"],
    });

    expect(searchIndexIsSettling(data)).toBe(false);
  });

  it("still waits for work that is already running when pause is enabled", () => {
    const data = appData({
      settings: { indexing_paused: true },
      jobs: [{ status: "running" }] as AppData["jobs"],
    });

    expect(searchIndexIsSettling(data)).toBe(true);
  });
});
