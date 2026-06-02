import { useEffect, useMemo, useState } from "react";

import { getTerminalSignalKey, useTerminalSignalsStore } from "../stores";
import type { TerminalSignal } from "../stores/useTerminalSignalsStore";
import type { Actor } from "../types";
import { getActorDisplayWorkingState, IDLE_PROMPT_TTL_MS, WORKING_OUTPUT_TTL_MS } from "../utils/terminalWorkingState";
import { getActorTabIndicatorState, type ActorTabIndicator } from "../components/tabBarIndicator";

export type ActorDisplayState = {
  isRunning: boolean;
  assumeRunning: boolean;
  workingState: string;
  indicator: ActorTabIndicator;
};

type UseActorDisplayStateInput = {
  groupId: string;
  actor: Actor;
  selectedGroupRunning?: boolean;
  selectedGroupActorsHydrating?: boolean;
};

const TERMINAL_SIGNAL_REFRESH_SKEW_MS = 50;

export function getTerminalSignalRefreshDelayMs(
  signal: TerminalSignal | null | undefined,
  nowMs: number = Date.now(),
): number | null {
  if (!signal) return null;
  const ttlMs = signal.kind === "idle_prompt"
    ? IDLE_PROMPT_TTL_MS
    : signal.kind === "working_output"
      ? WORKING_OUTPUT_TTL_MS
      : 0;
  if (ttlMs <= 0) return null;
  return Math.max(0, signal.updatedAt + ttlMs + TERMINAL_SIGNAL_REFRESH_SKEW_MS - nowMs);
}

export function useActorDisplayState({
  groupId,
  actor,
  selectedGroupRunning = false,
  selectedGroupActorsHydrating = false,
}: UseActorDisplayStateInput): ActorDisplayState {
  const terminalSignal = useTerminalSignalsStore((state) => state.signals[getTerminalSignalKey(groupId, actor.id)]);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const delayMs = getTerminalSignalRefreshDelayMs(terminalSignal);
    if (delayMs === null) return;
    const timer = window.setTimeout(() => {
      setNow(Date.now());
    }, delayMs);
    return () => window.clearTimeout(timer);
  }, [terminalSignal]);

  return useMemo(() => {
    const runningKnown = typeof actor.running === "boolean";
    const isRunning = runningKnown ? actor.running : (actor.enabled ?? false);
    const assumeRunning = !runningKnown && selectedGroupRunning && selectedGroupActorsHydrating && actor.enabled !== false;
    const workingState = getActorDisplayWorkingState(actor, terminalSignal, now);
    const indicator = getActorTabIndicatorState({
      isRunning: Boolean(isRunning),
      workingState,
      assumeRunning,
    });

    return {
      isRunning: Boolean(isRunning),
      assumeRunning,
      workingState,
      indicator,
    };
  }, [actor, now, selectedGroupActorsHydrating, selectedGroupRunning, terminalSignal]);
}
