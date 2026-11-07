import type { MouseEvent, SyntheticEvent } from "react";

/** Ctrl, Cmd, Shift, Alt and non-primary clicks stay the browser's own. */
export function isModifiedClick(event: MouseEvent) {
  return event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey;
}

/** One piece of message media plays at a time. A voice call is not message media. */
export function pauseOtherMessageMedia(event: SyntheticEvent<HTMLMediaElement>) {
  const current = event.currentTarget;
  for (const media of current.ownerDocument.querySelectorAll<HTMLMediaElement>(
    "[data-message-media]",
  )) {
    if (media !== current && !media.paused) media.pause();
  }
}
