import { afterEach, describe, expect, it, vi } from "vitest";
import { cloudClient } from "./client";

describe("cloud share media uploads", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("keeps bearer auth for absolute account-origin upload URLs", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    await cloudClient.uploadShareMedia(
      "account-token",
      "https://accounts.cerul.ai/v1/shares/share-1/media/clip",
      new Blob(["clip"], { type: "video/mp4" }),
    );

    expect(fetchMock).toHaveBeenCalledWith(
      "https://accounts.cerul.ai/v1/shares/share-1/media/clip",
      expect.objectContaining({
        headers: {
          authorization: "Bearer account-token",
          "content-type": "video/mp4",
        },
      }),
    );
  });

  it("does not forward bearer auth to signed external upload URLs", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    await cloudClient.uploadShareMedia(
      "account-token",
      "https://bucket.r2.cloudflarestorage.com/share-1/poster?signature=opaque",
      new Blob(["poster"], { type: "image/jpeg" }),
    );

    expect(fetchMock).toHaveBeenCalledWith(
      "https://bucket.r2.cloudflarestorage.com/share-1/poster?signature=opaque",
      expect.objectContaining({ headers: { "content-type": "image/jpeg" } }),
    );
  });
});
