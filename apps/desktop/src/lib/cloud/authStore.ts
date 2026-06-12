import { create } from "zustand";
import { cloudClient } from "./client";
import { CloudApiError, type CloudUser, type LoginInput, type OAuthExchangeInput, type RegisterInput } from "./types";
import { loadDesktopStore, type DesktopStore } from "../desktopHost";

// Persistence mirrors lib/uiStore.ts: desktop shell store with a localStorage
// fallback so it degrades outside the desktop shell.
const authStorePath = "cloud-auth.json";
const fallbackKey = "cerul.cloudAuth.v1";

interface PersistedAuth {
  accessToken: string | null;
  user: CloudUser | null;
}

const emptyAuth: PersistedAuth = { accessToken: null, user: null };

let storePromise: Promise<DesktopStore | null> | null = null;

async function loadAuthStore() {
  storePromise ??= loadDesktopStore(authStorePath).catch(() => null);
  return storePromise;
}

async function readPersisted(): Promise<PersistedAuth> {
  const store = await loadAuthStore();
  if (store) {
    return {
      accessToken: (await store.get<string>("accessToken")) ?? null,
      user: (await store.get<CloudUser>("user")) ?? null,
    };
  }
  try {
    const raw = window.localStorage.getItem(fallbackKey);
    return raw ? (JSON.parse(raw) as PersistedAuth) : emptyAuth;
  } catch {
    return emptyAuth;
  }
}

async function writePersisted(value: PersistedAuth) {
  const store = await loadAuthStore();
  if (store) {
    await store.set("accessToken", value.accessToken);
    await store.set("user", value.user);
    await store.save();
    return;
  }
  try {
    window.localStorage.setItem(fallbackKey, JSON.stringify(value));
  } catch {
    // best-effort; the in-memory session still works for this run
  }
}

export type AuthStatus = "loading" | "signedOut" | "signedIn";

interface AuthState {
  status: AuthStatus;
  user: CloudUser | null;
  accessToken: string | null;
  hydrate: () => Promise<void>;
  register: (input: RegisterInput) => Promise<void>;
  login: (input: LoginInput) => Promise<void>;
  exchangeOAuthCode: (input: OAuthExchangeInput) => Promise<void>;
  logout: () => Promise<void>;
  refreshMe: () => Promise<void>;
  sendVerificationCode: () => Promise<void>;
  verifyEmail: (code: string) => Promise<CloudUser>;
}

function isUnauthorized(error: unknown): boolean {
  return error instanceof CloudApiError && error.status === 401;
}

export const useAuthStore = create<AuthState>()((set, get) => ({
  status: "loading",
  user: null,
  accessToken: null,

  // Called once on launch. Restores the cached session, then validates the
  // token against /me in the background (clears it on 401, keeps it if offline).
  hydrate: async () => {
    const { accessToken, user } = await readPersisted();
    if (!accessToken) {
      set({ status: "signedOut", user: null, accessToken: null });
      return;
    }
    set({ status: "signedIn", accessToken, user });
    try {
      const { user: fresh } = await cloudClient.me(accessToken);
      set({ user: fresh });
      await writePersisted({ accessToken, user: fresh });
    } catch (error) {
      if (isUnauthorized(error)) {
        await writePersisted(emptyAuth);
        set({ status: "signedOut", user: null, accessToken: null });
      }
      // Other errors (offline): keep the optimistic signed-in state.
    }
  },

  register: async (input) => {
    const res = await cloudClient.register(input);
    await writePersisted({ accessToken: res.access_token, user: res.user });
    set({ status: "signedIn", accessToken: res.access_token, user: res.user });
  },

  login: async (input) => {
    const res = await cloudClient.login(input);
    await writePersisted({ accessToken: res.access_token, user: res.user });
    set({ status: "signedIn", accessToken: res.access_token, user: res.user });
  },

  exchangeOAuthCode: async (input) => {
    const res = await cloudClient.exchangeOAuth(input);
    await writePersisted({ accessToken: res.access_token, user: res.user });
    set({ status: "signedIn", accessToken: res.access_token, user: res.user });
  },

  logout: async () => {
    const token = get().accessToken;
    if (token) {
      try {
        await cloudClient.logout(token);
      } catch {
        // revoke best-effort; clear locally regardless
      }
    }
    await writePersisted(emptyAuth);
    set({ status: "signedOut", user: null, accessToken: null });
  },

  refreshMe: async () => {
    const token = get().accessToken;
    if (!token) return;
    try {
      const { user } = await cloudClient.me(token);
      set({ user });
      await writePersisted({ accessToken: token, user });
    } catch (error) {
      if (isUnauthorized(error)) {
        await writePersisted(emptyAuth);
        set({ status: "signedOut", user: null, accessToken: null });
      }
    }
  },

  sendVerificationCode: async () => {
    const token = get().accessToken;
    if (!token) throw new CloudApiError("not_signed_in", "not signed in", 401);
    await cloudClient.sendVerificationCode(token);
  },

  verifyEmail: async (code) => {
    const token = get().accessToken;
    if (!token) throw new CloudApiError("not_signed_in", "not signed in", 401);
    const { user } = await cloudClient.verifyEmail(token, code);
    set({ user });
    await writePersisted({ accessToken: token, user });
    return user;
  },
}));
