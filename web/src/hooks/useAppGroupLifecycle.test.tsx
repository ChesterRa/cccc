import { describe, expect, it } from "vitest";

import { shouldResetDestGroupForLifecycle } from "./useAppGroupLifecycle";

describe("useAppGroupLifecycle destination group reset", () => {
  it("does not reset an explicit cross-group destination during normal composing", () => {
    expect(shouldResetDestGroupForLifecycle({
      selectedGroupId: "g-current",
      destGroupId: "g-target",
      sendGroupId: "g-target",
      hasReplyTarget: false,
      hasComposerFiles: false,
    })).toBe(false);
  });

  it("resets a cross-group destination when reply or file state blocks cross-group sends", () => {
    expect(shouldResetDestGroupForLifecycle({
      selectedGroupId: "g-current",
      destGroupId: "g-target",
      sendGroupId: "g-target",
      hasReplyTarget: true,
      hasComposerFiles: false,
    })).toBe(true);
  });
});
