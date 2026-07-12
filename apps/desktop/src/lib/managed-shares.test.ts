import { describe, expect, it } from "vitest";
import type { PublishedShareResponse } from "./cloud/types";
import { markManagedShareRevoked, readManagedShares, recordManagedShare } from "./managed-shares";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
  };
}

function published(id: string, publishedAt: string): PublishedShareResponse {
  return {
    share_url: `https://app.cerul.ai/s/${id}`,
    share: {
      id,
      title: `Title ${id}`,
      headline: `Headline ${id}`,
      summary: "Summary",
      source_label: "Local video",
      shared_by: "Jessy",
      language: "zh",
      clip_url: `https://api.cerul.ai/v1/shares/${id}/media/clip`,
      poster_url: `https://api.cerul.ai/v1/shares/${id}/media/poster`,
      created_at: publishedAt,
      published_at: publishedAt,
    },
  };
}

describe("managed shares", () => {
  it("records shares newest first without duplicates", () => {
    const storage = memoryStorage();
    recordManagedShare(published("old", "2026-07-10T10:00:00.000Z"), storage);
    recordManagedShare(published("new", "2026-07-11T10:00:00.000Z"), storage);
    recordManagedShare(published("old", "2026-07-12T10:00:00.000Z"), storage);

    expect(readManagedShares(storage).map((share) => share.id)).toEqual(["old", "new"]);
  });

  it("keeps revoked shares in the local ledger", () => {
    const storage = memoryStorage();
    recordManagedShare(published("share-1", "2026-07-11T10:00:00.000Z"), storage);

    const [share] = markManagedShareRevoked("share-1", storage);

    expect(share?.status).toBe("revoked");
    expect(share?.revoked_at).toBeTruthy();
  });
});
