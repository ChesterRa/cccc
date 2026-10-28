import { createContext } from "react";
import type { Summary } from "./usePendingSummary";

export const PendingSummaryContext = createContext<{
  summaries: Record<string, Summary>;
  refresh: () => void;
} | null>(null);

/** Navigate only after the workspace's unsaved-changes guard succeeds. */
export const PendingReplyNavigationContext = createContext<((groupId: string) => void) | null>(
  null,
);
