// URL validation + small helpers shared by onboarding and AddSourceDialog.
// Extracted from App.tsx (B13 Phase E).

import type { TFunction } from "./i18n";
import type { OnboardingYoutubeChannel, ValidationState } from "./types";

export function validateHttpUrl(value: string, t: TFunction, allowedHosts?: string[]) {
  try {
    const parsed = new URL(value.trim());
    const protocolOk = parsed.protocol === "https:" || parsed.protocol === "http:";
    if (!protocolOk) {
      return { ok: false as const, message: t("validation.url.protocol") };
    }

    const hostname = parsed.hostname.replace(/^www\./, "");
    if (allowedHosts && !allowedHosts.some((host) => hostname === host || hostname.endsWith(`.${host}`))) {
      return { ok: false as const, message: t("validation.url.host", { hosts: allowedHosts.join(" / ") }) };
    }

    return { ok: true as const, hostname };
  } catch {
    return { ok: false as const, message: t("validation.url.invalid") };
  }
}

export type WebVideoPlatform = "youtube" | "bilibili";
export type WebVideoSourceKind = "single" | "author";

export type WebVideoClassification = {
  url: string;
  hostname: string;
  platform: WebVideoPlatform;
  sourceKind: WebVideoSourceKind;
};

export function classifyWebVideoUrl(value: string, t: TFunction) {
  const result = validateHttpUrl(value, t, ["youtube.com", "youtu.be", "bilibili.com", "b23.tv"]);
  if (!result.ok) {
    return result;
  }

  const parsed = new URL(value.trim());
  const hostname = parsed.hostname.replace(/^www\./, "").toLowerCase();
  const parts = parsed.pathname.split("/").filter(Boolean);
  const first = parts[0] ?? "";

  if (hostname === "youtu.be") {
    if (!first) {
      return { ok: false as const, message: t("addSource.webVideo.unsupported") };
    }
    return webVideoOk(parsed.toString(), result.hostname, "youtube", "single");
  }

  if (hostname === "youtube.com" || hostname.endsWith(".youtube.com")) {
    const hasVideoId = parsed.searchParams.has("v") && Boolean(parsed.searchParams.get("v")?.trim());
    const hasPlaylist = parsed.pathname === "/playlist" || parsed.searchParams.has("list");
    if (hasPlaylist && !hasVideoId) {
      return { ok: false as const, message: t("addSource.webVideo.playlistUnsupported") };
    }
    if ((first === "watch" && hasVideoId) || (["shorts", "live"].includes(first) && parts.length >= 2)) {
      return webVideoOk(parsed.toString(), result.hostname, "youtube", "single");
    }
    if (first.startsWith("@") || ["channel", "c", "user"].includes(first)) {
      return webVideoOk(ensureUrlPathSuffix(parsed, "videos"), result.hostname, "youtube", "author");
    }
    return { ok: false as const, message: t("addSource.webVideo.unsupportedYoutube") };
  }

  if (hostname === "b23.tv") {
    return webVideoOk(parsed.toString(), result.hostname, "bilibili", "single");
  }

  if (hostname === "bilibili.com" || hostname.endsWith(".bilibili.com")) {
    if (first === "video" && parts.length >= 2) {
      return webVideoOk(parsed.toString(), result.hostname, "bilibili", "single");
    }
    if (hostname === "space.bilibili.com" && first) {
      return webVideoOk(ensureUrlPathSuffix(parsed, "video"), result.hostname, "bilibili", "author");
    }
    return { ok: false as const, message: t("addSource.webVideo.unsupportedBilibili") };
  }

  return { ok: false as const, message: t("addSource.webVideo.unsupported") };
}

function webVideoOk(
  url: string,
  hostname: string,
  platform: WebVideoPlatform,
  sourceKind: WebVideoSourceKind,
) {
  return { ok: true as const, url, hostname, platform, sourceKind };
}

function ensureUrlPathSuffix(url: URL, suffix: string) {
  const parts = url.pathname.split("/").filter(Boolean);
  if (parts[parts.length - 1] !== suffix) {
    parts.push(suffix);
  }
  url.pathname = `/${parts.join("/")}`;
  url.search = "";
  url.hash = "";
  return url.toString();
}

export function youtubeChannelFromUrl(value: string, t: TFunction): OnboardingYoutubeChannel {
  const parsed = new URL(value.trim());
  const pathParts = parsed.pathname.split("/").filter(Boolean);
  const rawName = pathParts[0] ?? parsed.hostname.replace(/^www\./, "");
  const name = rawName.startsWith("@") ? rawName : `@${rawName}`;

  return {
    url: parsed.toString(),
    name,
    subscribers: t("validation.subscribersSync"),
  };
}

export function waitForValidationFrame() {
  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, 180);
  });
}

export function uniqueYoutubeChannels(channels: OnboardingYoutubeChannel[]) {
  const seen = new Set<string>();
  return channels.filter((channel) => {
    const key = channel.url.trim();
    if (!key || seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

export function addSourceDisabled(
  tab: "folder" | "file" | "youtube" | "podcast",
  folderPath: string,
  filePaths: string[],
  youtubeUrl: string,
  rssUrl: string,
  youtubeValidation: ValidationState,
  rssValidation: ValidationState,
) {
  if (tab === "folder") {
    return !folderPath.trim();
  }
  if (tab === "file") {
    return filePaths.length === 0;
  }
  if (tab === "youtube") {
    return (
      !youtubeUrl.trim() ||
      youtubeValidation.status === "validating" ||
      youtubeValidation.status === "error"
    );
  }
  return !rssUrl.trim() || rssValidation.status === "validating" || rssValidation.status === "error";
}
