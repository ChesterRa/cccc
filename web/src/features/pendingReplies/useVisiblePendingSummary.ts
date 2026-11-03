import { useContext } from "react";
import { useUIStore } from "../../stores/useUIStore";
import { PendingSummaryContext } from "./context";

/** The pending-reply summary where its indicators are shown; null where they are turned off. */
export function useVisiblePendingSummary() {
  const summary = useContext(PendingSummaryContext);
  const shown = useUIStore((state) => state.showPendingPrompts);
  return shown ? summary : null;
}
