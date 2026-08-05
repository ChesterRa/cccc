export function mapContainedImagePoint(
  point: { x: number; y: number },
  element: { left: number; top: number; width: number; height: number },
  frame: { width: number; height: number },
): { x: number; y: number } | null {
  if (element.width <= 0 || element.height <= 0 || frame.width <= 0 || frame.height <= 0) {
    return null;
  }
  const scale = Math.min(element.width / frame.width, element.height / frame.height);
  const contentWidth = frame.width * scale;
  const contentHeight = frame.height * scale;
  const contentLeft = element.left + (element.width - contentWidth) / 2;
  const contentTop = element.top + (element.height - contentHeight) / 2;
  const x = point.x - contentLeft;
  const y = point.y - contentTop;
  if (x < 0 || x > contentWidth || y < 0 || y > contentHeight) return null;
  return { x: Math.round(x / scale), y: Math.round(y / scale) };
}
