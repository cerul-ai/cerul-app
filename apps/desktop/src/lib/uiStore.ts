import { loadDesktopStore, type DesktopStore } from "./desktopHost";

const uiStateStorePath = "ui-state.json";
const localFallbackKey = "cerul.uiState.v1";

export type PersistedRoute = {
  view: string;
  itemId?: string | null;
  playbackChunkId?: string | null;
  timestamp?: string | null;
  settingsSection?: string | null;
  origin?: "results" | "library" | null;
};

export type PersistedUiState = {
  lastRoute?: PersistedRoute;
  // Set once the user finishes (or re-runs) the onboarding wizard. When this is
  // unset — a fresh install or after clearing local data — the app auto-opens
  // onboarding on launch so the first-run intro is always shown.
  hasCompletedOnboarding?: boolean;
  // Set true when the wizard hands off to the home screen, marking the cohort
  // that should see the first-run home guidance (the "now what?" indexing-wait
  // takeover + the ready-state getting-started banner). Resolved to false on the
  // first search or when the banner is dismissed, so it never shows to
  // established users (who never had it set) or twice to a new one.
  firstRunActive?: boolean;
};

let storePromise: Promise<DesktopStore | null> | null = null;

export async function loadPersistedUiState(): Promise<PersistedUiState> {
  const store = await loadUiStore();
  if (store) {
    return {
      lastRoute: await store.get<PersistedRoute>("lastRoute"),
      hasCompletedOnboarding: await store.get<boolean>("hasCompletedOnboarding"),
      firstRunActive: await store.get<boolean>("firstRunActive"),
    };
  }

  return readLocalFallback();
}

export async function persistLastRoute(route: PersistedRoute) {
  await persistUiPatch({ lastRoute: route });
}

export async function persistOnboardingCompleted(hasCompletedOnboarding: boolean) {
  await persistUiPatch({ hasCompletedOnboarding });
}

export async function persistFirstRunActive(firstRunActive: boolean) {
  await persistUiPatch({ firstRunActive });
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
