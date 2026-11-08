import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";

/** One request by the user to show a workspace file, and the panel state it was made in. */
export type WorkspaceTabChoice = { readonly generation: number };

/**
 * Decides which workspace file becomes the selected tab. Each file the user asks for records a
 * choice, and only that request, when its file arrives while it is still the latest choice and
 * nothing else was picked in the panel meanwhile, selects its tab. Any other file the editor ends
 * up holding, such as the tab in front being loaded again, only gets a tab: it cannot use another
 * request's choice, or two tabs would keep taking the selection from each other.
 */
export function useWorkspaceTabChoice(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  editorPath: string,
) {
  const latest = useRef<WorkspaceTabChoice | null>(null);
  const choose = useCallback((): WorkspaceTabChoice => {
    const choice = { generation: useResourcePanelStore.getState().generation };
    latest.current = choice;
    return choice;
  }, []);
  /** Adds the file's tab, and selects it when `select` holds. */
  const place = useCallback(
    (path: string, select: boolean) => {
      if (!groupId) return;
      const descriptor = { kind: "workspace" as const, scopeKey, scopeUrl, path };
      const title = path.split("/").pop() || path;
      const store = useResourcePanelStore.getState();
      if (select) store.open(groupId, descriptor, title);
      else store.ensure(groupId, descriptor, title);
    },
    [groupId, scopeKey, scopeUrl],
  );
  const settle = useCallback(
    (path: string, choice: WorkspaceTabChoice | null) => {
      const generation = useResourcePanelStore.getState().generation;
      const chosen = !!choice && latest.current === choice && choice.generation === generation;
      if (chosen) latest.current = null;
      place(path, chosen);
    },
    [place],
  );
  /**
   * The tab in front was loaded again and its file now lives at another path, as after it became a
   * symlink. That path's tab takes the front unless something was picked in the panel since the
   * load began (`since`); loading the old path again would only lead there again.
   */
  const follow = useCallback(
    (path: string, since: number) =>
      place(path, since === useResourcePanelStore.getState().generation),
    [place],
  );
  const settleRef = useRef(settle);
  useLayoutEffect(() => {
    settleRef.current = settle;
  }, [settle]);
  useEffect(() => {
    if (editorPath) settleRef.current(editorPath, null);
  }, [editorPath]);
  return { choose, settle, follow };
}
