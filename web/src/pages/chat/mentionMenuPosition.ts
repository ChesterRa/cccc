const MENU_GUTTER = 8;

export interface MentionMenuPositionInput {
  triggerX: number;
  containerWidth: number;
  menuWidth: number;
}

export function getMentionMenuLeft({ triggerX, containerWidth, menuWidth }: MentionMenuPositionInput): number {
  const maxLeft = Math.max(MENU_GUTTER, containerWidth - menuWidth - MENU_GUTTER);
  const preferredLeft = triggerX - menuWidth / 2 + MENU_GUTTER / 2;
  return Math.round(Math.min(Math.max(preferredLeft, MENU_GUTTER), maxLeft));
}

export function getMentionTriggerX(textarea: HTMLTextAreaElement, text: string): number {
  const style = window.getComputedStyle(textarea);
  const font = [
    style.fontStyle,
    style.fontVariant,
    style.fontWeight,
    style.fontSize,
    style.fontFamily,
  ].filter(Boolean).join(" ");
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return textarea.clientWidth / 2;
  context.font = font;

  const paddingLeft = parseFloat(style.paddingLeft) || 0;
  const paddingRight = parseFloat(style.paddingRight) || 0;
  const usableWidth = Math.max(1, textarea.clientWidth - paddingLeft - paddingRight);
  const currentLine = text.slice(text.lastIndexOf("\n") + 1);
  const measuredX = paddingLeft + context.measureText(currentLine).width - textarea.scrollLeft;
  return Math.min(Math.max(measuredX, MENU_GUTTER), textarea.clientWidth - MENU_GUTTER || usableWidth);
}
