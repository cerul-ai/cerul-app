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
  "search",
  "home",
  "results",
  "result-detail",
  "library",
  "moments",
  "entity-detail",
  "item-detail",
  "sources",
  "settings",
  "onboarding",
];

export function readRouteState(): RouteState {
  const raw = window.location.hash.replace(/^#/, "");
  const [id, queryString = ""] = raw.split("?");
  const params = new URLSearchParams(queryString);

  const rawView = VIEW_IDS.includes(id as View) ? (id as View) : "search";
  const view =
    rawView === "result-detail"
      ? "item-detail"
      : rawView === "home" || rawView === "results"
        ? "search"
      : rawView;

  return {
    view,
    itemId: params.get("itemId"),
    chunkId: params.get("chunkId"),
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
    chunkId?: string | null;
    timestamp?: string | null;
    settingsSection?: string | null;
  } = {},
) {
  const search = new URLSearchParams();
  if (params.itemId) {
    search.set("itemId", params.itemId);
  }
  if (params.chunkId) {
    search.set("chunkId", params.chunkId);
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
