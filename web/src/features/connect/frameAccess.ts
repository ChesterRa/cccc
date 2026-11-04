import { useSyncExternalStore } from "react";

type FrameState = "outside" | "active" | "paused";

let state: FrameState = "outside";
let deadline = 0;
let previousVisibility = "";
const listeners = new Set<() => void>();

function publish(next: FrameState) {
  if (state === next) return;
  state = next;
  for (const listener of [...listeners]) listener();
}

// This is a lifecycle guard for the embedded page, not an authorization
// boundary. The server independently checks the fresh frame proof.
export function beginFrameAccess(expiresAt: string) {
  deadline = Date.parse(expiresAt);
  publish("active");
}

export function pauseFrameAccess() {
  if (state !== "active") return;
  // Blurring only the iframe element in the parent does not prevent a focused
  // control in the child from receiving keys or reclaiming focus.
  if (typeof document !== "undefined") {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    previousVisibility = document.body.style.visibility;
    document.body.style.visibility = "hidden";
    document.body.inert = true;
  }
  publish("paused");
}

export function resumeFrameAccess(expiresAt: string) {
  deadline = Date.parse(expiresAt);
  if (typeof document !== "undefined" && state === "paused") {
    document.body.inert = false;
    document.body.style.visibility = previousVisibility;
  }
  publish("active");
}

export function endFrameAccess() {
  deadline = 0;
  if (typeof document !== "undefined" && state === "paused") {
    document.body.inert = false;
    document.body.style.visibility = previousVisibility;
  }
  publish("outside");
}

export function frameAccessPaused(): boolean {
  // Timers can be delayed while a laptop sleeps or a tab is throttled. Check
  // the deadline again at each entry point before starting new work.
  if (state === "active" && (!Number.isFinite(deadline) || Date.now() >= deadline)) {
    pauseFrameAccess();
  }
  return state === "paused";
}

export function subscribeFrameAccess(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useFrameAccessPaused() {
  return useSyncExternalStore(
    subscribeFrameAccess,
    () => state === "paused",
    () => false,
  );
}
