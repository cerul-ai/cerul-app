// Hash-route helpers. Extracted from App.tsx (B13 Phase E).
//
// The view component is the entire identity of the route; query-string
// params carry the secondary state (selected item, deep-link timestamp,
// settings section). Both `readRouteState` and `routeHash` round-trip
// losslessly so reloads land back where you were.

import type { RouteState, View } from "./types";

// All valid View ids — broader than the sidebar so persisted routes for
// sub-pages (result-detail, item-detail) and onboarding rehydrate.
const VIEW_IDS: View[] = [
  "home",
  "results",
  "result-detail",
  "library",
  "moments",
  "item-detail",
  "sources",
  "settings",
  "onboarding",
];

export function readRouteState(): RouteState {
  const raw = window.location.hash.replace(/^#/, "");
  const [id, queryString = ""] = raw.split("?");
  const params = new URLSearchParams(queryString);

  return {
    view: VIEW_IDS.includes(id as View) ? (id as View) : "home",
    itemId: params.get("itemId"),
    playbackChunkId: params.get("playbackChunkId") ?? params.get("chunkId"),
    timestamp: params.get("t"),
    settingsSection: params.get("section"),
    oauthProvider: params.get("provider"),
    oauthCode: params.get("code"),
    oauthState: params.get("state"),
    oauthError: params.get("error"),
  };
}

export function routeHash(
  view: View,
  params: {
    itemId?: string | null;
    playbackChunkId?: string | null;
    timestamp?: string | null;
    settingsSection?: string | null;
  } = {},
) {
  const search = new URLSearchParams();
  if (params.itemId) {
    search.set("itemId", params.itemId);
  }
  if (params.playbackChunkId) {
    search.set("playbackChunkId", params.playbackChunkId);
  }
  if (params.timestamp) {
    search.set("t", params.timestamp);
  }
  if (params.settingsSection) {
    search.set("section", params.settingsSection);
  }
  const query = search.toString();
  return query ? `${view}?${query}` : view;
}
