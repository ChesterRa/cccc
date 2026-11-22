import { FileText, Image as ImageGlyph, Presentation, type LucideIcon } from "lucide-react";
import type { ResourceTab } from "../../stores/useResourcePanelStore";
import type { LcdGlyphName } from "../LcdGlyph";

/**
 * One icon per kind of tab, shared by the tab strip and the "all tabs" list. An attached image,
 * SVG included, gets the image glyph; other attached files, such as a PDF, get the file one.
 */
export function resourceTabIcon(tab: ResourceTab): { icon: LucideIcon; glyph: LcdGlyphName } {
  if (tab.kind === "presentation") return { icon: Presentation, glyph: "monitor" };
  if (tab.kind === "attachment" && tab.media === "image")
    return { icon: ImageGlyph, glyph: "image" };
  return { icon: FileText, glyph: "file-text" };
}
