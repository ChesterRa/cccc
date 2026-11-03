import { useContext } from "react";
import { useUIStore } from "../../stores/useUIStore";
import { PendingSummaryContext } from "./context";
import { useWaitingFilterIndicator } from "./useWaitingFilterIndicator";

/** Presentation only: keep the provider and reply state alive while hints are hidden. */
export function useVisiblePendingSummary() {
  const summary = useContext(PendingSummaryContext);
  const visible = useUIStore((state) => state.showPendingReplyReminders);
  const prompts = useUIStore((state) => state.showPendingPrompts);
  return visible && prompts ? summary : null;
}

/** The local reminder preference does not change notification offers; upstream quiet mode does. */
export function usePendingReplyIndicators(groupId: string, active: boolean) {
  const indicator = useWaitingFilterIndicator(groupId, active);
  const visible = useUIStore((state) => state.showPendingReplyReminders);
  return {
    waitingIndicator: visible ? indicator : null,
    hasPendingReplies: (indicator?.count ?? 0) > 0,
  };
}
