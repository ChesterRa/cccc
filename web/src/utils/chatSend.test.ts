import { describe, expect, it } from "vite-plus/test";

import { formatSendMessageError, shouldBlockLocalCrossGroupAttachments } from "./chatSend";

it("distinguishes an unconfirmed response from a confirmed rejection", () => {
  for (const code of ["NETWORK_ERROR", "EMPTY_RESPONSE", "PARSE_ERROR"]) {
    expect(formatSendMessageError({ code, message: "fetch failed", t: (key) => key })).toBe(
      "sendResultUnknown",
    );
  }
  expect(
    formatSendMessageError({ code: "permission_denied", message: "Not allowed", t: (key) => key }),
  ).toBe("permission_denied: Not allowed");
});

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
