const lastOpenedStorageKey = "cerul.lastOpened.v1";

export type LastOpenedItem = {
  itemId: string;
  timestamp: string | null;
  at: number;
};

export function recordLastOpened(itemId: string, timestamp?: string | null) {
  try {
    window.localStorage.setItem(
      lastOpenedStorageKey,
      JSON.stringify({ itemId, timestamp: timestamp ?? null, at: Date.now() }),
    );
  } catch {
    // localStorage may be unavailable; continue-watching is best-effort.
  }
}

export function forgetLastOpened(itemId: string) {
  try {
    const current = readLastOpened();
    if (!current || current.itemId === itemId) {
      window.localStorage.removeItem(lastOpenedStorageKey);
    }
  } catch {
    // localStorage may be unavailable; continue-watching is best-effort.
  }
}

export function readLastOpened(): LastOpenedItem | null {
  try {
    const raw = window.localStorage.getItem(lastOpenedStorageKey);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { itemId?: unknown; timestamp?: unknown; at?: unknown };
    if (parsed && typeof parsed.itemId === "string") {
      return {
        itemId: parsed.itemId,
        timestamp: typeof parsed.timestamp === "string" ? parsed.timestamp : null,
        at: typeof parsed.at === "number" && Number.isFinite(parsed.at) ? parsed.at : 0,
      };
    }
  } catch {
    // ignore malformed storage
  }
  return null;
}
