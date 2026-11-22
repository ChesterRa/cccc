import { expect, it } from "vite-plus/test";
import type { ResourceTab } from "../../stores/useResourcePanelStore";
import { resourceTabIcon } from "./resourceTabIcon";

const tab = (fields: Record<string, unknown>) => fields as unknown as ResourceTab;

it("gives attached images the image glyph and every other file the file glyph", () => {
  expect(resourceTabIcon(tab({ kind: "attachment", media: "image" })).glyph).toBe("image");
  expect(resourceTabIcon(tab({ kind: "attachment", media: "image", isSvg: true })).glyph).toBe(
    "image",
  );
  for (const media of ["pdf", "video", "markdown", "html", "text"]) {
    expect(resourceTabIcon(tab({ kind: "attachment", media })).glyph).toBe("file-text");
  }
  expect(resourceTabIcon(tab({ kind: "workspace" })).glyph).toBe("file-text");
  expect(resourceTabIcon(tab({ kind: "presentation" })).glyph).toBe("monitor");
});
