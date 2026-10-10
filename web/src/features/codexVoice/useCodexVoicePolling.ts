import { useEffect, type RefObject } from "react";
import type { CodexVoiceAnalystInfo } from "../../services/api";

export function useCodexVoicePolling(args: {
  enabled: boolean;
  analyst: CodexVoiceAnalystInfo | null;
  owned: boolean;
  sessionRef: RefObject<unknown | null>;
  refresh(showChecking?: boolean): Promise<void>;
}) {
  const { enabled, analyst, owned, sessionRef, refresh } = args;
  const generation = analyst?.generation;
  const phase = analyst?.phase;

  // TUI work has no browser-owned call socket. Poll only after an Analyst exists, and keep the
  // cadence bounded; this exposes terminal turns without another event protocol or a permanent
  // poll for users who never use Codex Voice.
  useEffect(() => {
    if (!enabled || !generation || owned || sessionRef.current) return;
    let cancelled = false;
    let timer = 0;
    const poll = async () => {
      if (document.visibilityState === "visible" && !sessionRef.current) {
        await refresh(false);
      }
      if (!cancelled) {
        timer = window.setTimeout(poll, phase === "working" ? 1_500 : 5_000);
      }
    };
    timer = window.setTimeout(poll, phase === "working" ? 1_500 : 5_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // Fast ACP snapshots must not postpone this owner's call/readiness read.
    // Ownership changes also re-arm polling when an owned call ends.
  }, [enabled, generation, owned, phase, refresh, sessionRef]);
}
