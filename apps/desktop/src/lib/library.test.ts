import { describe, expect, it } from "vitest";
import { sortLibraryItems } from "./library";
import type { Item } from "./types";

function item(id: string, addedAtEpoch: number | null, indexedAtEpoch: number | null): Item {
  return {
    id,
    title: id,
    addedAtEpoch,
    indexedAtEpoch,
  } as Item;
}

describe("sortLibraryItems", () => {
  it("sorts recently added items by added time before indexing finishes", () => {
    const freshQueued = item("fresh", 300, null);
    const olderIndexed = item("older", 200, 250);

    expect([olderIndexed, freshQueued].sort((a, b) => sortLibraryItems(a, b, "recent")))
      .toEqual([freshQueued, olderIndexed]);
  });

  it("falls back to indexed time for pre-migration records", () => {
    const newer = item("newer", null, 200);
    const older = item("older", null, 100);

    expect([older, newer].sort((a, b) => sortLibraryItems(a, b, "recent")))
      .toEqual([newer, older]);
  });
});
