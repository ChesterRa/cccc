import { describe, expect, it } from "vitest";

import { getComposerActionVisibility, getComposerCanSend } from "./chatComposerActions";

describe("ChatComposer action visibility", () => {
  it("hides message mode selector on small screens", () => {
    expect(getComposerActionVisibility(true)).toEqual({
      showMessageModeSelector: false,
    });
  });

  it("keeps message mode selector on larger screens", () => {
    expect(getComposerActionVisibility(false)).toEqual({
      showMessageModeSelector: true,
    });
  });
});

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

  it("disables send while recipients are still resolving", () => {
    expect(getComposerCanSend({ composerText: "hello", composerFilesCount: 0, recipientResolutionBusy: true })).toBe(false);
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 1, recipientResolutionBusy: true })).toBe(false);
  });
});
