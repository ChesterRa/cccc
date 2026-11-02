import { useContext } from "react";
import { useUIStore } from "../../stores/useUIStore";
import { PendingSummaryContext } from "./context";
import { pendingFilterIndicator, type PendingIndicator } from "./indicator";

/**
 * The in-Group Waiting for you filter, or null where it is not offered. With
 * those prompts off, requests to the user read as ordinary messages: no tab,
 * no count and no offer to turn on notifications for them.
 */
export function useWaitingFilterIndicator(
  groupId: string,
  active: boolean,
): PendingIndicator | null {
  const context = useContext(PendingSummaryContext);
  const shown = useUIStore((state) => state.showPendingPrompts);
  if (!context || !groupId || !shown) return null;
  return pendingFilterIndicator(context.summaries[groupId], active);
}
