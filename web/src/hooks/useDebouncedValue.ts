import { useEffect, useState } from "react";

export function scheduleDebounced(callback: () => void, delayMs: number): () => void {
  const timer = globalThis.setTimeout(callback, delayMs);
  return () => globalThis.clearTimeout(timer);
}

export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => scheduleDebounced(() => setDebounced(value), delayMs), [delayMs, value]);

  return debounced;
}
