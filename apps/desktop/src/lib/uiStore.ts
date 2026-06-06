import { loadDesktopStore, type DesktopStore } from "./desktopHost";

const uiStateStorePath = "ui-state.json";
const localFallbackKey = "cerul.uiState.v1";

export type PersistedRoute = {
  view: string;
  itemId?: string | null;
  chunkId?: string | null;
  timestamp?: string | null;
  settingsSection?: string | null;
};

export type PersistedUiState = {
  lastRoute?: PersistedRoute;
  sidebarCollapsed?: boolean;
};

let storePromise: Promise<DesktopStore | null> | null = null;

export async function loadPersistedUiState(): Promise<PersistedUiState> {
  const store = await loadUiStore();
  if (store) {
    return {
      lastRoute: await store.get<PersistedRoute>("lastRoute"),
      sidebarCollapsed: await store.get<boolean>("sidebarCollapsed"),
    };
  }

  return readLocalFallback();
}

export async function persistLastRoute(route: PersistedRoute) {
  await persistUiPatch({ lastRoute: route });
}

export async function persistSidebarCollapsed(sidebarCollapsed: boolean) {
  await persistUiPatch({ sidebarCollapsed });
}

async function persistUiPatch(patch: PersistedUiState) {
  const store = await loadUiStore();
  if (store) {
    for (const [key, value] of Object.entries(patch)) {
      await store.set(key, value);
    }
    await store.save();
    return;
  }

  writeLocalFallback({
    ...readLocalFallback(),
    ...patch,
  });
}

async function loadUiStore() {
  storePromise ??= loadDesktopStore(uiStateStorePath).catch(() => null);
  return storePromise;
}

function readLocalFallback(): PersistedUiState {
  try {
    const raw = window.localStorage.getItem(localFallbackKey);
    return raw ? (JSON.parse(raw) as PersistedUiState) : {};
  } catch {
    return {};
  }
}

function writeLocalFallback(state: PersistedUiState) {
  try {
    window.localStorage.setItem(localFallbackKey, JSON.stringify(state));
  } catch {
    // UI state persistence is best-effort and must not block navigation.
  }
}
