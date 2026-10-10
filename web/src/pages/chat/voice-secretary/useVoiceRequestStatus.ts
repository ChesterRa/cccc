import { useEffect } from "react";
import { fetchVoiceAssistantStatus } from "../../../services/api";
import type { AssistantStateResult } from "../../../types";

/** Observe an owning request; elapsed time and hiding its hint never settle work. */
export function useVoiceRequestStatus({
  groupId,
  requestId,
  enabled,
  kind = "prompt",
  version = 0,
  onStatus,
  onError,
}: {
  groupId: string;
  requestId: string;
  enabled: boolean;
  kind?: "prompt" | "ask";
  version?: number;
  onStatus: (groupId: string, requestId: string, result: AssistantStateResult) => void;
  onError: (groupId: string, requestId: string, message: string) => void;
}) {
  useEffect(() => {
    if (!enabled || !groupId || !requestId) return;
    let cancelled = false;
    let inFlight = false;
    const poll = async () => {
      if (cancelled || inFlight) return;
      inFlight = true;
      try {
        const response = await fetchVoiceAssistantStatus(
          groupId,
          kind === "prompt" ? { promptRequestId: requestId } : undefined,
        );
        if (cancelled) return;
        if (response.ok) onStatus(groupId, requestId, response.result);
        else onError(groupId, requestId, response.error.message);
      } catch (error) {
        if (!cancelled)
          onError(groupId, requestId, error instanceof Error ? error.message : String(error));
      } finally {
        inFlight = false;
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled, groupId, requestId, kind, version, onStatus, onError]);
}
