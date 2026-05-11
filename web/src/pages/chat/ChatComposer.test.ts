import { describe, expect, it } from "vitest";

import { getComposerActionVisibility } from "./chatComposerActions";

describe("ChatComposer action visibility", () => {
  it("hides PET shortcut and message mode selector on small screens", () => {
    expect(getComposerActionVisibility(true)).toEqual({
      showPetShortcut: false,
      showMessageModeSelector: false,
    });
  });

  it("keeps PET shortcut and message mode selector on larger screens", () => {
    expect(getComposerActionVisibility(false)).toEqual({
      showPetShortcut: true,
      showMessageModeSelector: true,
    });
  });
});
