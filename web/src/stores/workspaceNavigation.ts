import { create } from "zustand";

// Only dirty flags and a pending user action cross component boundaries. File
// contents stay in the scope-owned editor; nothing is persisted in the browser.
export const useWorkspaceNavigation = create<{
  owners: Set<symbol>;
  pending: { proceed: () => void; focus: HTMLElement | null } | null;
}>(() => ({ owners: new Set(), pending: null }));

export function setWorkspaceDirty(owner: symbol, dirty: boolean) {
  useWorkspaceNavigation.setState((state) => {
    if (state.owners.has(owner) === dirty) return state;
    const owners = new Set(state.owners);
    if (dirty) owners.add(owner);
    else owners.delete(owner);
    // A forced scope/authority change retires the old navigation as well.
    return { owners, pending: owners.size ? state.pending : null };
  });
}

export function requestWorkspaceNavigation(proceed: () => void) {
  const state = useWorkspaceNavigation.getState();
  if (!state.owners.size) {
    proceed();
    return;
  }
  if (!state.pending)
    useWorkspaceNavigation.setState({
      pending: {
        proceed,
        focus: document.activeElement instanceof HTMLElement ? document.activeElement : null,
      },
    });
}

export function finishWorkspaceNavigation(discard: boolean) {
  const pending = useWorkspaceNavigation.getState().pending;
  useWorkspaceNavigation.setState({ pending: null });
  if (discard) pending?.proceed();
}
