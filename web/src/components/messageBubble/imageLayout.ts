export const MESSAGE_IMAGE_PREVIEW_HEIGHT_PX = { hero: 224, grid: 128 } as const;

export const MESSAGE_IMAGE_GRID_GAP_PX = 8;
export const MESSAGE_ATTACHMENT_SECTION_LEADING_PX = 25;
export const MESSAGE_ATTACHMENT_STACK_GAP_PX = 12;

export function getMessageImageGridHeight(imageCount: number): number {
  const rows = Math.ceil(Math.max(0, imageCount) / 2);
  if (rows === 0) return 0;
  return rows * MESSAGE_IMAGE_PREVIEW_HEIGHT_PX.grid + (rows - 1) * MESSAGE_IMAGE_GRID_GAP_PX;
}
