import { useEffect, type RefObject } from "react";
import type { CodexVoiceBrowserSession } from "./codexVoiceSession";

export function useCodexVoiceWindowLifecycle(args: {
  refresh(): Promise<void>;
  mountedRef: RefObject<boolean>;
  refreshGenerationRef: RefObject<number>;
  sessionRef: RefObject<CodexVoiceBrowserSession | null>;
}) {
  const { refresh, mountedRef, refreshGenerationRef, sessionRef } = args;

  useEffect(() => {
    void refresh();
    const refreshOnFocus = () => void refresh();
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    window.addEventListener("focus", refreshOnFocus);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refreshOnFocus);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [refresh]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      refreshGenerationRef.current += 1;
      const session = sessionRef.current;
      sessionRef.current = null;
      if (session) void session.stop();
    };
  }, [mountedRef, refreshGenerationRef, sessionRef]);
}
