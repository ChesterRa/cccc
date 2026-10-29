import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();
let tick = 0;
let timer: number | undefined;

function subscribe(listener: () => void) {
  listeners.add(listener);
  // One timer serves every subscriber and stops with the last one.
  timer ??= window.setInterval(() => {
    tick += 1;
    for (const notify of listeners) notify();
  }, 60_000);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.clearInterval(timer);
      timer = undefined;
    }
  };
}

/** Renders the caller again each minute, for text such as "5 minutes ago". */
export function useMinuteClock(): void {
  useSyncExternalStore(subscribe, () => tick);
}
