import { create } from "zustand";

// The page owns these operations; components may unmount before they settle.
// Only identities are stored here, never message text, files or audio.
export const usePendingPageWork = create<{ owners: ReadonlySet<symbol> }>(() => ({
  owners: new Set(),
}));

/** Register one operation and return its idempotent release. */
export function beginPendingPageWork(): () => void {
  const owner = Symbol("pending page work");
  usePendingPageWork.setState((state) => ({ owners: new Set([...state.owners, owner]) }));
  return () => {
    usePendingPageWork.setState((state) => {
      if (!state.owners.has(owner)) return state;
      const owners = new Set(state.owners);
      owners.delete(owner);
      return { owners };
    });
  };
}
