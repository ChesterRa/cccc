import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
export function useWorkspaceFiles(groupId: string, active: boolean) {
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
  // A group switch invalidates everything - the tree, the open file, and any save in flight.
  useEffect(() => {
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree(emptyTreeState());
  }, [groupId]);

  // The ignored-file filter only decides which entries the tree lists. Unsaved edits belong
  // to the editor, so they survive a toggle.
  useEffect(() => {
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree(emptyTreeState());
  }, [showIgnored]);

  const pending = useMemo(
    () => (active && groupId ? pendingDirectories(tree) : []),
    [active, groupId, tree],
  );

  useEffect(() => {
    if (!pending.length) return;
    const token = listingGeneration.current;
    for (const path of pending) {
      if (inFlight.current.has(path)) continue;
      inFlight.current.add(path);
      setTree((current) => setDirectory(current, path, { loading: true, error: "" }));
      void api.fetchWorkspaceListing(groupId, path, { showIgnored }).then((response) => {
        inFlight.current.delete(path);
        if (token !== listingGeneration.current) return;
        if (!response.ok) {
          setTree((current) =>
            setDirectory(current, path, { loading: false, error: response.error.message }),
          );
          return;
        }
        if (path === ROOT_PATH) setRootPath(response.result.root_path);
        setTree((current) =>
          setDirectory(current, path, { items: response.result.items, loading: false, error: "" }),
        );
      });
    }
  }, [groupId, pending, showIgnored]);

  const rows = useMemo(() => flattenTree(tree), [tree]);

  const toggleDirectory = useCallback((path: string) => {
    setTree((current) => toggleExpanded(current, path));
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
  const editor = useWorkspaceEditor(groupId, refresh, onOpenPath);
  return { rows, rootPath, tree, showIgnored, setShowIgnored, toggleDirectory, refresh, ...editor };
}
