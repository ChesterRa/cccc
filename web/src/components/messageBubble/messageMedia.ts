import type { SyntheticEvent } from "react";

/** One piece of message media plays at a time. A voice call is not message media. */
export function pauseOtherMessageMedia(event: SyntheticEvent<HTMLMediaElement>) {
  const current = event.currentTarget;
  for (const media of current.ownerDocument.querySelectorAll<HTMLMediaElement>(
    "[data-message-media]",
  )) {
    if (media !== current && !media.paused) media.pause();
  }
}
