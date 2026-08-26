import { describe, expect, it } from "vite-plus/test";

import { voicePromptRequestOwnership } from "./voiceComposerUtils";

describe("voicePromptRequestOwnership", () => {
  it("allows reuse only inside the owning Group", () => {
    const request = {
      requestId: "request-a",
      pendingGroupId: "group-a",
      startedAt: 1_000,
      nowMs: 2_000,
    };

    expect(voicePromptRequestOwnership({ ...request, targetGroupId: "group-a" })).toBe(
      "same_group",
    );
    expect(voicePromptRequestOwnership({ ...request, targetGroupId: "group-b" })).toBe(
      "other_group",
    );
  });

  it("does not retain ownership after the request expires", () => {
    expect(
      voicePromptRequestOwnership({
        requestId: "request-a",
        pendingGroupId: "group-a",
        targetGroupId: "group-b",
        startedAt: 1_000,
        nowMs: 181_001,
      }),
    ).toBe("none");
  });
});
