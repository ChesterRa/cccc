import { describe, expect, it } from "vite-plus/test";

import { shouldBlockLocalCrossGroupAttachments } from "./chatSend";

describe("shouldBlockLocalCrossGroupAttachments", () => {
  it("blocks attachment sends to local cross-group targets even when replying", () => {
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: true }],
      }),
    ).toBe(true);
  });

  it("allows attachments within the selected instance Group", () => {
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: false }],
      }),
    ).toBe(false);
  });
});
