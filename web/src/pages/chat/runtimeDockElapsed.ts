import { useEffect, useRef, useState } from "react";

const ELAPSED_TICK_MS = 10_000;

export function useElapsedNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), ELAPSED_TICK_MS);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}

export function useStateSinceMs(
  isRunning: boolean,
  workingState: string,
  backendSinceIso: string | null | undefined,
  stoppedSinceLedgerMs: number,
): { ms: number; stopped: boolean } | null {
  const stateKey = isRunning ? String(workingState || "running") : "stopped";
  const prevKeyRef = useRef<string | null>(null);
  const baselineRef = useRef<{ key: string; at: number } | null>(null);
  const flipRef = useRef<{ key: string; at: number } | null>(null);
  if (prevKeyRef.current === null) {
    baselineRef.current = { key: stateKey, at: Date.now() };
  } else if (prevKeyRef.current !== stateKey) {
    const at = Date.now();
    baselineRef.current = { key: stateKey, at };
    flipRef.current = { key: stateKey, at };
  }
  prevKeyRef.current = stateKey;

  if (stateKey === "stopped") {
    const flipMs = flipRef.current?.key === "stopped" ? flipRef.current.at : 0;
    const ms = Math.max(stoppedSinceLedgerMs > 0 ? stoppedSinceLedgerMs : 0, flipMs);
    return ms > 0 ? { ms, stopped: true } : null;
  }
  const backendSinceMs = Date.parse(String(backendSinceIso || ""));
  const baseline = baselineRef.current?.at ?? Date.now();
  const ms = Number.isFinite(backendSinceMs) ? Math.min(backendSinceMs, baseline) : baseline;
  return { ms, stopped: false };
}
