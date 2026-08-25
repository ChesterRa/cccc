export type RefreshActorsFn = (
  groupId: string,
  opts?: { includeUnread?: boolean },
) => Promise<unknown>;

export type FetchContextOverviewFn = (
  groupId: string,
  opts: { detail: "overview"; fresh?: boolean },
) => Promise<unknown>;

export async function runReconnectCatchup(
  groupId: string,
  deps: {
    invalidateContextRead: (groupId: string) => void;
    reconcileLedgerTail: (groupId: string) => Promise<unknown>;
    refreshActors: RefreshActorsFn;
    fetchContextOverview: FetchContextOverviewFn;
  },
) {
  deps.invalidateContextRead(groupId);
  await Promise.allSettled([
    deps.reconcileLedgerTail(groupId),
    deps.refreshActors(groupId, { includeUnread: false }),
    // Client cache has already been invalidated above; avoid forcing `fresh=1`
    // here so concurrent consumers can still reuse the shared overview read.
    deps.fetchContextOverview(groupId, { detail: "overview" }),
  ]);
  await deps.refreshActors(groupId, { includeUnread: true });
}

export function scheduleContextOverviewCatchup(
  groupId: string,
  deps: {
    invalidateContextRead: (groupId: string) => void;
    existingTimer: number | null;
    clearTimer: (id: number) => void;
    setTimer: (cb: () => void, delayMs: number) => number;
    fetchContextOverview: (groupId: string, opts: { detail: "overview"; fresh?: boolean }) => void;
  },
) {
  deps.invalidateContextRead(groupId);
  if (deps.existingTimer !== null) {
    deps.clearTimer(deps.existingTimer);
  }
  return deps.setTimer(() => {
    deps.fetchContextOverview(groupId, { detail: "overview" });
  }, 150);
}
