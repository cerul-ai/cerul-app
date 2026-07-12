import type { PublishedShareResponse, PublicShare } from "./cloud/types";

const managedSharesStorageKey = "cerul.managedShares.v1";

export type ManagedShareStatus = "active" | "revoked";

export type ManagedShare = PublicShare & {
  share_url: string;
  status: ManagedShareStatus;
  revoked_at: string | null;
  identity?: ManagedShareIdentity;
};

export type ManagedShareIdentity = {
  itemId: string;
  chunkId: string;
  timestamp: string;
};

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function browserStorage(): StorageLike | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function isManagedShare(value: unknown): value is ManagedShare {
  if (!value || typeof value !== "object") return false;
  const share = value as Partial<ManagedShare>;
  return (
    typeof share.id === "string" &&
    typeof share.title === "string" &&
    typeof share.headline === "string" &&
    typeof share.share_url === "string" &&
    (share.status === "active" || share.status === "revoked")
  );
}

export function readManagedShares(storage: StorageLike | null = browserStorage()): ManagedShare[] {
  if (!storage) return [];
  try {
    const raw = storage.getItem(managedSharesStorageKey);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(isManagedShare)
      .sort((left, right) => Date.parse(right.published_at) - Date.parse(left.published_at));
  } catch {
    return [];
  }
}

function writeManagedShares(shares: ManagedShare[], storage: StorageLike | null) {
  if (!storage) return;
  try {
    storage.setItem(managedSharesStorageKey, JSON.stringify(shares));
  } catch {
    // Share creation and revocation still succeed if local metadata cannot be persisted.
  }
}

export function recordManagedShare(
  response: PublishedShareResponse,
  identity?: ManagedShareIdentity,
  storage: StorageLike | null = browserStorage(),
): ManagedShare[] {
  const current = readManagedShares(storage);
  const next: ManagedShare = {
    ...response.share,
    share_url: response.share_url,
    status: "active",
    revoked_at: null,
    identity,
  };
  const shares = [next, ...current.filter((share) => share.id !== next.id)];
  writeManagedShares(shares, storage);
  return shares;
}

export function markManagedShareRevoked(
  shareId: string,
  storage: StorageLike | null = browserStorage(),
): ManagedShare[] {
  const revokedAt = new Date().toISOString();
  const shares = readManagedShares(storage).map((share) =>
    share.id === shareId ? { ...share, status: "revoked" as const, revoked_at: revokedAt } : share,
  );
  writeManagedShares(shares, storage);
  return shares;
}

export function managedSharesStorageEventKey() {
  return managedSharesStorageKey;
}
