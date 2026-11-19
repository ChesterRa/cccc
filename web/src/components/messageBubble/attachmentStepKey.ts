import type { KeyboardEvent } from "react";
import type { AttachmentResourceMedia } from "../resourcePanel/views/AttachmentResourceView";

const TYPING =
  'input, textarea, select, video, audio, [contenteditable]:not([contenteditable="false"])';

/**
 * The step an unmodified left or right arrow asks for, or 0 when the key belongs to what has
 * focus. On the bar around an attachment both arrows step. Inside the attachment, matched by
 * `content`, only an image leaves them free, and not once it is zoomed in far enough to scroll
 * sideways. Keys pressed inside a PDF or HTML frame never reach the page at all.
 */
export function attachmentStepKey(
  event: KeyboardEvent<HTMLElement>,
  media: AttachmentResourceMedia,
  content: string,
): -1 | 0 | 1 {
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey)
    return 0;
  const delta = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0;
  const target = event.target;
  if (!delta || !(target instanceof HTMLElement) || target.closest(TYPING)) return 0;
  if (!target.closest(content)) return delta;
  return media === "image" && target.scrollWidth <= target.clientWidth ? delta : 0;
}
