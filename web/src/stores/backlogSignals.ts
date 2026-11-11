import { create } from "zustand";

/**
 * Counts, per Group, the ledger events that said its backlog file changed and the stream
 * reconnects after which such an event may have been missed. A backlog view reads the file
 * again when its Group's count moves; nothing here holds file contents.
 */
export const useBacklogSignals = create<{ revisions: Record<string, number> }>(() => ({
  revisions: {},
}));

export function signalBacklogChanged(groupId: string) {
  if (!groupId) return;
  useBacklogSignals.setState((state) => ({
    revisions: { ...state.revisions, [groupId]: (state.revisions[groupId] || 0) + 1 },
  }));
}
