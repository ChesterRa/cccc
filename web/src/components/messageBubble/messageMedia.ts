import type { MouseEvent, SyntheticEvent } from "react";

/** Ctrl, Cmd, Shift, Alt and non-primary clicks stay the browser's own. */
export function isModifiedClick(
  event: Pick<MouseEvent, "button" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
) {
  return event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey;
}

/** Format playback time in seconds as m:ss or h:mm:ss. */
export function formatAudioTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0:00";
  const whole = Math.floor(seconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const secs = whole % 60;
  const paddedSecs = String(secs).padStart(2, "0");
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${paddedSecs}`;
  }
  return `${minutes}:${paddedSecs}`;
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
