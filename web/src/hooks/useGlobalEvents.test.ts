import { describe, expect, it } from "vite-plus/test";

import { shouldRefreshActorsAfterGlobalEvent } from "./globalEventRefreshPolicy";

describe("useGlobalEvents actor status refresh", () => {
  it("does not refetch actors for activity events handled by the group ledger stream", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.activity", group_id: "g_active" },
        "g_active",
      ),
    ).toBe(false);
  });

  it("still refreshes actors for selected-group lifecycle changes", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.start", group_id: "g_active" },
        "g_active",
      ),
    ).toBe(true);
  });
});
