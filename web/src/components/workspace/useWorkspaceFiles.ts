import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import * as api from "../../services/api";
import { useWorkspaceEditor } from "./useWorkspaceEditor";
import {
  ROOT_PATH,
  ancestorsOf,
  emptyTreeState,
  expandPaths,
  flattenTree,
  invalidateDirectories,
  pendingDirectories,
  setDirectory,
  toggleExpanded,
  type TreeState,
} from "./workspaceTreeModel";

export type WorkspaceFilesController = ReturnType<typeof useWorkspaceFiles>;

/**
 * Owns the lazily loaded directory cache and the currently opened file.
 *
 * Directories are fetched one level at a time as the user expands them, so opening the panel
 * on a large repository costs a single listing.
 */
export function useWorkspaceFiles(
  groupId: string,
  active: boolean,
  scopeKey: string,
  scopeUrl: string,
) {
  const [tree, setTree] = useState<TreeState>(emptyTreeState);
  const [showIgnored, setShowIgnored] = useState(false);
  const [rootPath, setRootPath] = useState("");
  // Guards the load effect against re-entering a directory that is already in flight.
  const inFlight = useRef(new Set<string>());
  // Bumped whenever the listings on screen stop describing what the tree should show.
  // Responses from an older generation are dropped here rather than by an effect cleanup:
  // the load effect re-runs on every `pending` change, so cleanup-based cancellation would
  // abort the request it just started.
  const listingGeneration = useRef(0);
  // A Group or scope change retires the tree, editor, and pending responses.
  useLayoutEffect(() => {
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree(emptyTreeState());
    setRootPath("");
  }, [groupId, scopeKey, scopeUrl]);

  // The ignored-file filter only decides which entries the tree lists. Unsaved edits belong
  // to the editor, so they survive a toggle.
  useEffect(() => {
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree(emptyTreeState());
  }, [showIgnored]);

  const pending = useMemo(
    () => (active && groupId && scopeKey && scopeUrl ? pendingDirectories(tree) : []),
    [active, groupId, scopeKey, scopeUrl, tree],
  );

  useEffect(() => {
    if (!pending.length) return;
    const token = listingGeneration.current;
    for (const path of pending) {
      if (inFlight.current.has(path)) continue;
      inFlight.current.add(path);
      setTree((current) => setDirectory(current, path, { loading: true, error: "" }));
      void api
        .fetchWorkspaceListing(groupId, path, { showIgnored, scopeKey, scopeUrl })
        .then((response) => {
          if (token !== listingGeneration.current) return;
          inFlight.current.delete(path);
          if (!response.ok) {
            setTree((current) =>
              setDirectory(current, path, { loading: false, error: response.error.message }),
            );
            return;
          }
          if (path === ROOT_PATH) setRootPath(response.result.root_path);
          setTree((current) =>
            setDirectory(current, path, {
              items: response.result.items,
              loading: false,
              error: "",
            }),
          );
        });
    }
  }, [groupId, scopeKey, scopeUrl, pending, showIgnored]);

  const rows = useMemo(() => flattenTree(tree), [tree]);

  const toggleDirectory = useCallback((path: string) => {
    setTree((current) => toggleExpanded(current, path));
  }, []);

  const retryDirectory = useCallback((path: string) => {
    setTree((current) => {
      const directories = { ...current.directories };
      delete directories[path];
      return { ...current, directories };
    });
  }, []);

  const refresh = useCallback(() => {
    // Retire the listings already in flight: they describe the tree being discarded, and
    // would otherwise land on top of the reload they were replaced by.
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree((current) => invalidateDirectories(current));
  }, []);

  const onOpenPath = useCallback((path: string) => {
    setTree((current) => expandPaths(current, ancestorsOf(path)));
  }, []);
  const editor = useWorkspaceEditor(groupId, scopeKey, scopeUrl, refresh, onOpenPath);
  return {
    rows,
    rootPath,
    scopeAvailable: !!scopeKey && !!scopeUrl,
    tree,
    showIgnored,
    setShowIgnored,
    toggleDirectory,
    retryDirectory,
    refresh,
    ...editor,
  };
}
