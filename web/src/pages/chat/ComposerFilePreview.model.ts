const IMAGE_FILE_EXTENSIONS = new Set([".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif"]);

function fileExtension(name: string): string {
  const clean = String(name || "").trim().toLowerCase();
  const dot = clean.lastIndexOf(".");
  return dot >= 0 ? clean.slice(dot) : "";
}

interface ComposerPreviewPositionArgs {
  anchor: Pick<DOMRect, "left" | "right" | "top" | "bottom" | "width" | "height">;
  viewport: { width: number; height: number };
  preview?: { width: number; height: number };
  gap?: number;
  margin?: number;
}

export function getComposerPreviewPosition({
  anchor,
  viewport,
  preview = { width: 224, height: 230 },
  gap = 8,
  margin = 12,
}: ComposerPreviewPositionArgs): { left: number; top: number } {
  const minLeft = margin;
  const maxLeft = Math.max(minLeft, viewport.width - preview.width - margin);
  const centeredLeft = anchor.left + (anchor.width - preview.width) / 2;
  const left = Math.min(Math.max(centeredLeft, minLeft), maxLeft);
  const preferredTop = anchor.top - preview.height - gap;
  const fallbackTop = anchor.bottom + gap;
  const maxTop = Math.max(margin, viewport.height - preview.height - margin);
  const top = preferredTop >= margin
    ? preferredTop
    : Math.min(Math.max(fallbackTop, margin), maxTop);
  return { left, top };
}

export function isPreviewableComposerImageFile(file: File): boolean {
  const type = String(file.type || "").trim().toLowerCase();
  if (type.startsWith("image/")) return true;
  return IMAGE_FILE_EXTENSIONS.has(fileExtension(file.name));
}
