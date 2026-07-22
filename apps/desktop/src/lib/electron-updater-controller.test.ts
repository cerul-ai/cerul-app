import { EventEmitter } from "node:events";

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createUpdaterController,
  type DesktopUpdateInfo,
} from "../../../electron-shell/src/updater";

class FakeUpdater extends EventEmitter {
  autoDownload = true;
  autoInstallOnAppQuit = true;
  checkForUpdates = vi.fn(async () => null);
  downloadUpdate = vi.fn(async () => [] as string[]);
}

function githubUpdate(version?: string): DesktopUpdateInfo | null {
  return version
    ? {
        version,
        url: `https://github.com/cerul-ai/cerul-app/releases/tag/v${version}`,
        prerelease: false,
        publishedAt: "2026-07-22T00:00:00Z",
        releaseNotes: {
          sections: [{ title: "Fixed", items: ["Update reliability."] }],
        },
      }
    : null;
}

function createController(
  updater: FakeUpdater,
  checkForReleaseUpdate = vi.fn(async (): Promise<DesktopUpdateInfo | null> => null),
) {
  return createUpdaterController({
    checkForReleaseUpdate,
    clearPreparedUpdate: vi.fn(),
    getMainWindow: () => null,
    isPackaged: () => true,
    resolveAutoUpdater: () => updater as never,
    markInstallWhenPrepared: vi.fn(),
    prepareDownloadedUpdateForRestart: vi.fn(),
    installUpdate: vi.fn(async () => undefined),
  });
}

describe("desktop updater controller", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps a background provider check error quiet when there is no update", async () => {
    const updater = new FakeUpdater();
    const networkError = new Error("net::ERR_NETWORK_CHANGED");
    updater.checkForUpdates.mockImplementation(async () => {
      updater.emit("error", networkError);
      throw networkError;
    });
    const controller = createController(updater);

    await expect(controller.runCheck()).resolves.toBe(true);
    expect(controller.getState()).toEqual({ phase: "idle" });
  });

  it("preserves the GitHub release fallback when the auto-update provider check fails", async () => {
    const updater = new FakeUpdater();
    const networkError = new Error("net::ERR_NETWORK_CHANGED");
    updater.checkForUpdates.mockImplementation(async () => {
      updater.emit("error", networkError);
      throw networkError;
    });
    const controller = createController(
      updater,
      vi.fn(async () => githubUpdate("0.0.66")),
    );

    await expect(controller.runCheck()).resolves.toBe(true);
    expect(controller.getState()).toMatchObject({
      phase: "available",
      version: "0.0.66",
      canAutoInstall: false,
    });
  });

  it("still surfaces a real update download failure", async () => {
    const updater = new FakeUpdater();
    updater.downloadUpdate.mockRejectedValue(new Error("download interrupted"));
    updater.checkForUpdates.mockImplementation(async () => {
      updater.emit("update-available", { version: "0.0.66" });
      return null;
    });
    const controller = createController(
      updater,
      vi.fn(async () => githubUpdate("0.0.66")),
    );

    await controller.runCheck();
    await vi.waitFor(() => {
      expect(controller.getState()).toMatchObject({
        phase: "error",
        version: "0.0.66",
        message: "download interrupted",
      });
    });
  });

  it("clears an old download error after a successful no-update check", async () => {
    const updater = new FakeUpdater();
    updater.downloadUpdate.mockRejectedValueOnce(new Error("download interrupted"));
    updater.checkForUpdates.mockImplementationOnce(async () => {
      updater.emit("update-available", { version: "0.0.66" });
      return null;
    });
    const checkForReleaseUpdate = vi
      .fn<() => Promise<DesktopUpdateInfo | null>>()
      .mockResolvedValueOnce(githubUpdate("0.0.66"))
      .mockResolvedValueOnce(githubUpdate());
    const controller = createController(updater, checkForReleaseUpdate);

    await controller.runCheck();
    await vi.waitFor(() => expect(controller.getState().phase).toBe("error"));

    updater.checkForUpdates.mockImplementationOnce(async () => {
      updater.emit("update-not-available", { version: "0.0.65" });
      return null;
    });
    await controller.runCheck();
    expect(controller.getState()).toEqual({ phase: "idle" });
  });
});
