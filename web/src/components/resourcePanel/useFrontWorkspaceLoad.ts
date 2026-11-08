import { useEffect } from "react";

/** What the shared workspace editor is doing, as the panel sees it. */
export type WorkspaceEditorActivity = {
  /** The file the editor holds, or "". */
  path: string;
  /** The file being read, or the one shown when no read is pending. */
  selectedPath: string;
  loading: boolean;
  /** A path from a message or the path box is being looked up. */
  pathLoading: boolean;
  error: string;
};

/**
 * Loads the file of the workspace tab in front when the shared editor holds another one, as after
 * the Group or its folder changed. It waits while a read or a path lookup is under way: that one
 * is the user's, and a new read would cancel it. A read of this file that failed is not repeated;
 * the tab offers to try again.
 */
export function useFrontWorkspaceLoad(
  frontPath: string,
  editor: WorkspaceEditorActivity,
  load: (path: string) => unknown,
) {
  const { path, selectedPath, loading, pathLoading, error } = editor;
  useEffect(() => {
    if (!frontPath || path === frontPath || loading || pathLoading) return;
    if (selectedPath === frontPath && error) return;
    void load(frontPath);
  }, [error, frontPath, load, loading, path, pathLoading, selectedPath]);
}
