import { describe, expect, it } from "vitest";

import { getGroupSendBlockedReason } from "./chatSend";

describe("getGroupSendBlockedReason", () => {
  it("does not block active groups with no running actors", () => {
    expect(getGroupSendBlockedReason({
      lifecycleState: "active",
      runtimeRunning: false,
      actorCount: 1,
    })).toBeNull();
  });

  it("blocks paused groups but allows stopped groups to auto-wake server-side", () => {
    expect(getGroupSendBlockedReason({
      lifecycleState: "paused",
      runtimeRunning: true,
      actorCount: 1,
    })).toBe("paused");
    expect(getGroupSendBlockedReason({
      lifecycleState: "stopped",
      runtimeRunning: false,
      actorCount: 1,
    })).toBeNull();
  });
});
