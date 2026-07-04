const recentSearchesStorageKey = "cerul.recentSearches.v1";

export function readOverlayRecentSearches() {
  try {
    const raw = window.localStorage.getItem(recentSearchesStorageKey);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed
        .filter((value): value is string => typeof value === "string" && value.trim().length > 0)
        .slice(0, 5)
      : [];
  } catch {
    return [];
  }
}
