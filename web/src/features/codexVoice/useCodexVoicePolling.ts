import { useEffect, type RefObject } from "react";
import type { CodexVoiceAnalystInfo } from "../../services/api";

export function useCodexVoicePolling(args: {
  enabled: boolean;
  analyst: CodexVoiceAnalystInfo | null;
  sessionRef: RefObject<unknown | null>;
  refresh(showChecking?: boolean): Promise<void>;
}) {
  const { enabled, analyst, sessionRef, refresh } = args;

  // TUI work has no browser-owned call socket. Poll only after an Analyst exists, and keep the
  // cadence bounded; this exposes terminal turns without another event protocol or a permanent
  // poll for users who never use Codex Voice.
  useEffect(() => {
    if (!enabled || !analyst || sessionRef.current) return;
    let cancelled = false;
    let timer = 0;
    const poll = async () => {
      if (document.visibilityState === "visible" && !sessionRef.current) {
        await refresh(false);
      }
      if (!cancelled) {
        timer = window.setTimeout(poll, analyst.phase === "working" ? 1_500 : 5_000);
      }
    };
    timer = window.setTimeout(poll, analyst.phase === "working" ? 1_500 : 5_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [analyst, enabled, refresh, sessionRef]);
}
