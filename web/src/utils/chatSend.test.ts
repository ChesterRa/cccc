import { describe, expect, it } from "vitest";

import { getGroupSendBlockedReason, shouldBlockLocalCrossGroupAttachments } from "./chatSend";

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

describe("shouldBlockLocalCrossGroupAttachments", () => {
  it("blocks attachment sends to local cross-group targets even when replying", () => {
    expect(shouldBlockLocalCrossGroupAttachments({
      attachmentCount: 1,
      targets: [{ isCrossGroup: true, isRemote: false }],
    })).toBe(true);
  });

  it("allows attachment sends when all cross-group targets are remote", () => {
    expect(shouldBlockLocalCrossGroupAttachments({
      attachmentCount: 1,
      targets: [{ isCrossGroup: true, isRemote: true }],
    })).toBe(false);
  });
});
