import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { useWorkspaceChanges } from "./useWorkspaceChanges";
import { useWorkspaceMutations } from "./useWorkspaceMutations";
import * as api from "../../services/api";
import { useWorkspaceEditor, type WorkspaceOpenFileOptions } from "./useWorkspaceEditor";
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
  const [mode, setMode] = useState<"files" | "changes">("files");
  const [revision, setRevision] = useState(0);
  const changes = useWorkspaceChanges(
    groupId,
    scopeKey,
    scopeUrl,
    active && mode === "changes",
    revision,
  );
  const [tree, setTree] = useState<TreeState>(emptyTreeState);
  const [showIgnored, updateShowIgnored] = useState(() => {
    try {
      return localStorage.getItem("cccc-workspace-show-ignored") !== "false";
    } catch {
      return true;
    }
  });
  const setShowIgnored = useCallback((value: boolean) => {
    updateShowIgnored(value);
    try {
      localStorage.setItem("cccc-workspace-show-ignored", String(value));
    } catch {
      // Browsing still works when the browser disallows preference storage.
    }
  }, []);
  const [revealRequest, setRevealRequest] = useState<{ path: string } | null>(null);
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
    setRevealRequest(null);
  }, [groupId, scopeKey, scopeUrl]);

  // The ignored-file filter only decides which entries the tree lists. Unsaved edits belong
  // to the editor, so they survive a toggle.
  useEffect(() => {
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree((current) => invalidateDirectories(current));
  }, [showIgnored]);

  const pending = useMemo(
    () =>
      active && mode === "files" && groupId && scopeKey && scopeUrl ? pendingDirectories(tree) : [],
    [active, mode, groupId, scopeKey, scopeUrl, tree],
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
    setRevision((value) => value + 1);
    // Retire the listings already in flight: they describe the tree being discarded, and
    // would otherwise land on top of the reload they were replaced by.
    listingGeneration.current += 1;
    inFlight.current.clear();
    setTree((current) => invalidateDirectories(current));
  }, []);

  const onOpenPath = useCallback((path: string) => {
    setTree((current) => expandPaths(current, ancestorsOf(path)));
  }, []);
  const revealFile = useCallback(
    (path: string, isDirectory = false) => {
      // Explicit reveal must also find a target hidden by the optional Git filter.
      setShowIgnored(true);
      setTree((current) =>
        expandPaths(current, [...ancestorsOf(path), ...(isDirectory && path ? [path] : [])]),
      );
      setRevealRequest({ path });
    },
    [setShowIgnored],
  );
  const onLocatePath = useCallback(
    (path: string, isDirectory: boolean) => {
      // A pasted path can name something created after the cached directory listing.
      refresh();
      revealFile(path, isDirectory);
    },
    [refresh, revealFile],
  );
  const editor = useWorkspaceEditor(groupId, scopeKey, scopeUrl, refresh, onOpenPath, onLocatePath);
  const mutations = useWorkspaceMutations(groupId, scopeKey, scopeUrl, editor, refresh);
  const {
    openFile: openEditorFile,
    locatePath: locateEditorPath,
    closeFile: closeEditorFile,
  } = editor;
  // A newer navigation retires the previous reveal, even while its directory is loading.
  const openFile = useCallback(
    (path: string, options?: WorkspaceOpenFileOptions) => {
      setMode("files");
      setRevealRequest(null);
      return openEditorFile(path, options);
    },
    [openEditorFile],
  );
  const locatePath = useCallback(
    (path: string) => {
      setRevealRequest(null);
      return locateEditorPath(path);
    },
    [locateEditorPath],
  );
  const closeFile = useCallback(() => {
    setRevealRequest(null);
    closeEditorFile();
  }, [closeEditorFile]);
  useEffect(() => {
    const root = tree.directories[ROOT_PATH];
    if (revealRequest?.path === ROOT_PATH && root && !root.loading && !root.items.length) {
      setRevealRequest(null);
    }
  }, [tree, revealRequest]);
  const completeReveal = useCallback(() => setRevealRequest(null), []);
  return {
    mode,
    setMode,
    changes,
    mutations,
    groupId,
    scopeKey,
    scopeUrl,
    rows,
    rootPath,
    scopeAvailable: !!scopeKey && !!scopeUrl,
    tree,
    showIgnored,
    setShowIgnored,
    toggleDirectory,
    retryDirectory,
    refresh,
    revealRequest,
    revealFile,
    completeReveal,
    collapseAll: () => {
      setRevealRequest(null);
      setTree((current) => ({ ...current, expanded: [] }));
    },
    ...editor,
    openFile,
    locatePath,
    closeFile,
  };
}
