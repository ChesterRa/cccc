import { describe, expect, it } from "vitest";

import { shouldRefreshGroupBridgePairingAfterGlobalEvent } from "./useGlobalEvents";

describe("useGlobalEvents Group Bridge pairing refresh", () => {
  it("refreshes the selected group when bridge access changes", () => {
    expect(shouldRefreshGroupBridgePairingAfterGlobalEvent({
      kind: "group_bridge.pairing.trust_access_updated",
      group_id: "g_active",
    }, "g_active")).toBe(true);
  });

  it("does not refresh another selected group for bridge access changes", () => {
    expect(shouldRefreshGroupBridgePairingAfterGlobalEvent({
      kind: "group_bridge.pairing.trust_access_updated",
      group_id: "g_other",
    }, "g_active")).toBe(false);
  });
});
