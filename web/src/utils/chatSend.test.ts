import { describe, expect, it } from "vite-plus/test";

import { shouldBlockLocalCrossGroupAttachments } from "./chatSend";

describe("shouldBlockLocalCrossGroupAttachments", () => {
  it("blocks attachment sends to local cross-group targets even when replying", () => {
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: true, isRemote: false }],
      }),
    ).toBe(true);
  });

  it("allows attachment sends when all cross-group targets are remote", () => {
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: true, isRemote: true }],
      }),
    ).toBe(false);
  });
});
