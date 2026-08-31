import { describe, expect, it } from "vite-plus/test";

import { getComposerCanSend, hasConcreteReplyRecipients } from "./chatComposerActions";

describe("ChatComposer send availability", () => {
  it("enables send when the composer has non-whitespace text", () => {
    expect(getComposerCanSend({ composerText: "hello", composerFilesCount: 0 })).toBe(true);
  });

  it("enables send when the composer only has files", () => {
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 1 })).toBe(true);
  });

  it("disables send when the composer has no text or files", () => {
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 0 })).toBe(false);
  });

  it("keeps send available while destination actor chips are still resolving", () => {
    expect(
      getComposerCanSend({
        composerText: "hello",
        composerFilesCount: 0,
        recipientResolutionBusy: true,
      }),
    ).toBe(true);
    expect(
      getComposerCanSend({
        composerText: "   ",
        composerFilesCount: 1,
        recipientResolutionBusy: true,
      }),
    ).toBe(true);
  });

  it("uses the local foreman by default for Send + Reply", () => {
    expect(hasConcreteReplyRecipients(["peer1"])).toBe(true);
    expect(hasConcreteReplyRecipients([])).toBe(true);
    expect(hasConcreteReplyRecipients(["@foreman"])).toBe(true);
    expect(hasConcreteReplyRecipients(["peer1"], true)).toBe(false);
    expect(
      getComposerCanSend({
        composerText: "please answer",
        composerFilesCount: 0,
        messageMode: "request_reply",
        toTokens: ["@all"],
      }),
    ).toBe(false);
    expect(
      getComposerCanSend({
        composerText: "please answer",
        composerFilesCount: 0,
        messageMode: "request_reply",
      }),
    ).toBe(true);
    expect(
      getComposerCanSend({
        composerText: "please answer",
        composerFilesCount: 0,
        messageMode: "request_reply",
        toTokens: ["peer1"],
      }),
    ).toBe(true);
  });
});
