import { useEffect } from "react";
import { fetchVoiceAssistantStatus } from "../../../services/api";
import type { AssistantStateResult } from "../../../types";

/** Observe the owning request until it produces a draft; elapsed time never settles work. */
export function useVoicePromptStatus({
  groupId,
  requestId,
  enabled,
  onStatus,
  onError,
}: {
  groupId: string;
  requestId: string;
  enabled: boolean;
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
        const response = await fetchVoiceAssistantStatus(groupId, { promptRequestId: requestId });
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
  }, [enabled, groupId, requestId, onStatus, onError]);
}
