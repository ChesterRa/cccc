const MENU_GUTTER = 8;

export interface MentionMenuPositionInput {
  triggerX: number;
  containerWidth: number;
  menuWidth: number;
}

export function getMentionMenuLeft({
  triggerX,
  containerWidth,
  menuWidth,
}: MentionMenuPositionInput): number {
  const maxLeft = Math.max(MENU_GUTTER, containerWidth - menuWidth - MENU_GUTTER);
  const preferredLeft = triggerX - menuWidth / 2 + MENU_GUTTER / 2;
  return Math.round(Math.min(Math.max(preferredLeft, MENU_GUTTER), maxLeft));
}

/**
 * Where a composer list opens when it is not typed into the text, such as the quick
 * reply list: above the input, like the mention menu. A wide (phone) list spans the
 * input with the menu gutters; otherwise it starts where the mention menu would for
 * the insertion point.
 */
export function getComposerMenuAnchorRect({
  textarea,
  wide,
  menuWidth,
}: {
  textarea: HTMLTextAreaElement;
  wide: boolean;
  menuWidth: number;
}): DOMRect {
  const box = (textarea.closest("[data-composer-input]") ?? textarea).getBoundingClientRect();
  if (wide) {
    return new DOMRect(
      box.left + MENU_GUTTER,
      box.top,
      Math.max(0, box.width - MENU_GUTTER * 2),
      0,
    );
  }
  const caret = textarea.selectionStart ?? textarea.value.length;
  const left = getMentionMenuLeft({
    triggerX: getMentionTriggerX(textarea, textarea.value.slice(0, caret)),
    containerWidth: textarea.clientWidth,
    menuWidth,
  });
  return new DOMRect(box.left + left, box.top, 0, 0);
}

export function getMentionTriggerX(textarea: HTMLTextAreaElement, text: string): number {
  const style = window.getComputedStyle(textarea);
  const font = [
    style.fontStyle,
    style.fontVariant,
    style.fontWeight,
    style.fontSize,
    style.fontFamily,
  ]
    .filter(Boolean)
    .join(" ");
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return textarea.clientWidth / 2;
  context.font = font;

  const paddingLeft = parseFloat(style.paddingLeft) || 0;
  const paddingRight = parseFloat(style.paddingRight) || 0;
  const usableWidth = Math.max(1, textarea.clientWidth - paddingLeft - paddingRight);
  const currentLine = text.slice(text.lastIndexOf("\n") + 1);
  const measuredX = paddingLeft + context.measureText(currentLine).width - textarea.scrollLeft;
  return Math.min(
    Math.max(measuredX, MENU_GUTTER),
    textarea.clientWidth - MENU_GUTTER || usableWidth,
  );
}
