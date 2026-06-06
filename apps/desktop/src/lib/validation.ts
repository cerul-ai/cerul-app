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
    return !youtubeUrl.trim() || youtubeValidation.status === "validating";
  }
  return !rssUrl.trim() || rssValidation.status === "validating";
}
