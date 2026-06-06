// Settings accessors and theme resolution. Extracted from App.tsx
// as part of the B13 audit follow-up.

import type * as api from "./api";

export function settingString(settings: api.SettingsMap, key: string, fallback: string) {
  const value = settings[key];
  return typeof value === "string" ? value : fallback;
}

export function settingBoolean(settings: api.SettingsMap, key: string, fallback: boolean) {
  const value = settings[key];
  return typeof value === "boolean" ? value : fallback;
}

export function settingNumber(settings: api.SettingsMap, key: string, fallback: number) {
  const value = settings[key];
  return typeof value === "number" ? value : fallback;
}

export function resolveThemePreference(theme: string, prefersLight: boolean) {
  if (theme === "Light") {
    return "light";
  }
  if (theme === "Dark") {
    return "dark";
  }
  return prefersLight ? "light" : "dark";
}

export function formatDownloadSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  const mb = bytes / 1_000_000;
  if (mb < 1) return `${(bytes / 1_000).toFixed(0)} KB`;
  if (mb < 1_000) return `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
  return `${(mb / 1_000).toFixed(2)} GB`;
}

export function formatDownloadRate(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "—";
  return `${formatDownloadSize(bytesPerSecond)}/s`;
}

export function formatDownloadEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "—";
  const total = Math.round(seconds);
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  const remaining = total % 60;
  if (minutes < 60) {
    return remaining === 0 ? `${minutes}m` : `${minutes}m ${remaining}s`;
  }
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  return mins === 0 ? `${hours}h` : `${hours}h ${mins}m`;
}
