import { describe, expect, it } from "vite-plus/test";

import {
  shouldRefreshActorsAfterGlobalEvent,
  shouldRefreshGroupBridgePairingAfterGlobalEvent,
} from "./globalEventRefreshPolicy";

describe("useGlobalEvents Group Bridge pairing refresh", () => {
  it("refreshes the selected group when bridge access changes", () => {
    expect(
      shouldRefreshGroupBridgePairingAfterGlobalEvent(
        { kind: "group_bridge.pairing.trust_access_updated", group_id: "g_active" },
        "g_active",
      ),
    ).toBe(true);
  });

  it("does not refresh another selected group for bridge access changes", () => {
    expect(
      shouldRefreshGroupBridgePairingAfterGlobalEvent(
        { kind: "group_bridge.pairing.trust_access_updated", group_id: "g_other" },
        "g_active",
      ),
    ).toBe(false);
  });
});

describe("useGlobalEvents actor status refresh", () => {
  it("refreshes the selected actor list for authoritative activity changes", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.activity", group_id: "g_active" },
        "g_active",
      ),
    ).toBe(true);
  });

  it("does not refresh a different group for activity changes", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.activity", group_id: "g_other" },
        "g_active",
      ),
    ).toBe(false);
  });
});
