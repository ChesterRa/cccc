import { annotate } from "rough-notation";
import type { RoughAnnotationConfig } from "rough-notation/lib/model";

export function createHighlighterAnnotation(element: HTMLElement, config: RoughAnnotationConfig) {
  const annotation = annotate(element, config);
  annotation.show();
  const svg = element.previousElementSibling;
  if (svg?.tagName.toLowerCase() === "svg" && svg.classList.contains("rough-annotation")) {
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
  }
  return annotation;
}
