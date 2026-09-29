import { useEffect, useRef, useState } from "react";

import type { LedgerEvent } from "../../types";

const ELAPSED_TICK_MS = 10_000;

const ACTOR_LIFECYCLE_KINDS = new Set([
  "actor.start",
  "actor.stop",
  "actor.restart",
  "actor.new_session",
]);

export function formatElapsedCompact(ms: number): string {
  const totalMin = Math.max(0, Math.floor(ms / 60000));
  if (totalMin < 60) return `${totalMin}m`;
  const totalHr = Math.floor(totalMin / 60);
  const remMin = totalMin % 60;
  return remMin ? `${totalHr}h${remMin}m` : `${totalHr}h`;
}

/**
 * Latest lifecycle outcome per actor: `number` ts when its most recent
 * lifecycle event is actor.stop, null when the most recent is a start/restart.
 * Actors absent from the map have no lifecycle event in the given window.
 */
export function buildActorStoppedSinceMap(events: LedgerEvent[]): Map<string, number | null> {
  const outcome = new Map<string, number | null>();
  if (!Array.isArray(events)) return outcome;
  for (let i = events.length - 1; i >= 0; i -= 1) {
    const ev = events[i];
    const kind = String(ev?.kind || "");
    if (!ACTOR_LIFECYCLE_KINDS.has(kind)) continue;
    const actorId = String((ev?.data as { actor_id?: unknown } | undefined)?.actor_id || "").trim();
    if (!actorId || outcome.has(actorId)) continue;
    const tsMs = kind === "actor.stop" ? Date.parse(String(ev?.ts || "")) : NaN;
    outcome.set(actorId, Number.isFinite(tsMs) ? tsMs : null);
  }
  return outcome;
}

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
