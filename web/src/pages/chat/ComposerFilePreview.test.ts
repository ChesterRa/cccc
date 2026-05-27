import { describe, expect, it } from "vitest";

import { getComposerPreviewPosition, isPreviewableComposerImageFile } from "./ComposerFilePreview";

describe("isPreviewableComposerImageFile", () => {
  it("only enables hover previews for image files", () => {
    expect(isPreviewableComposerImageFile(new File(["png"], "image.png", { type: "image/png" }))).toBe(true);
    expect(isPreviewableComposerImageFile(new File(["txt"], "notes.txt", { type: "text/plain" }))).toBe(false);
  });
});

describe("getComposerPreviewPosition", () => {
  it("keeps the floating image preview inside the viewport", () => {
    const position = getComposerPreviewPosition({
      anchor: { left: 90, right: 260, top: 460, bottom: 492, width: 170, height: 32 },
      viewport: { width: 360, height: 520 },
      preview: { width: 224, height: 230 },
      gap: 8,
      margin: 12,
    });

    expect(position.left).toBeGreaterThanOrEqual(12);
    expect(position.left + 224).toBeLessThanOrEqual(348);
    expect(position.top).toBeGreaterThanOrEqual(12);
    expect(position.top + 230).toBeLessThanOrEqual(508);
  });
});
