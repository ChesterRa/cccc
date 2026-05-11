import { describe, expect, it } from "vitest";

import { getComposerDestGroupDisplayValue } from "./useComposerStore";

describe("useComposerStore helpers", () => {
  it("shows the selected group while composer state is still switching groups", () => {
    expect(getComposerDestGroupDisplayValue("old-group", "new-group", false)).toBe("new-group");
  });
});
