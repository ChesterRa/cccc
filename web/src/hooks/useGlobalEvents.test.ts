import { describe, expect, it } from "vitest";

import { shouldRefreshFederationPairingAfterGlobalEvent } from "./useGlobalEvents";

describe("useGlobalEvents federation pairing refresh", () => {
  it("refreshes the selected group when bridge access changes", () => {
    expect(shouldRefreshFederationPairingAfterGlobalEvent({
      kind: "federation.pairing.trust_access_updated",
      group_id: "g_active",
    }, "g_active")).toBe(true);
  });

  it("does not refresh another selected group for bridge access changes", () => {
    expect(shouldRefreshFederationPairingAfterGlobalEvent({
      kind: "federation.pairing.trust_access_updated",
      group_id: "g_other",
    }, "g_active")).toBe(false);
  });
});
