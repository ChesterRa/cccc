import { useEffect, useMemo, useState, type ReactNode } from "react";
import { PendingSummaryContext } from "./context";
import { createPendingReplyStore, PendingReplyStoreContext } from "./store";
import { usePendingSummary } from "./usePendingSummary";

/** State is scoped to the authenticated app, not persisted in the browser. */
export function PendingReplyProvider({
  enabled,
  groupIds,
  children,
}: {
  enabled: boolean;
  groupIds: readonly string[];
  children: ReactNode;
}) {
  const [store] = useState(createPendingReplyStore);
  const { summaries, refresh } = usePendingSummary(enabled, groupIds);
  const denied = Object.values(summaries).some((summary) => summary.accessDenied);
  const value = useMemo(() => ({ summaries, refresh }), [summaries, refresh]);
  const key = JSON.stringify([...new Set(groupIds.filter(Boolean))].sort());
  useEffect(() => {
    if (enabled && !denied) store.getState().retainGroups(JSON.parse(key) as string[]);
    else store.getState().clearAll();
  }, [denied, enabled, key, store]);
  useEffect(() => () => store.getState().clearAll(), [store]);
  return (
    <PendingReplyStoreContext.Provider value={enabled && !denied ? store : null}>
      <PendingSummaryContext.Provider value={enabled && !denied ? value : null}>
        {children}
      </PendingSummaryContext.Provider>
    </PendingReplyStoreContext.Provider>
  );
}
