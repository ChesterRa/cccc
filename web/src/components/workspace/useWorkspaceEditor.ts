import { useCallback, useLayoutEffect, useRef, useState } from "react";
import * as api from "../../services/api";
import { finishWorkspaceNavigation, setWorkspaceDirty } from "../../stores/workspaceNavigation";
import type { WorkspaceFile } from "../../types";

export type WorkspaceOpenFileOptions = { reload?: boolean; fragment?: string };
export type WorkspaceFileNavigation = { fragment: string };

/** Scope keys and file names can themselves contain colons (for example on Windows). */
function scopeFileKey(scopeKey: string, scopeUrl: string, path: string): string {
  return JSON.stringify([scopeKey, scopeUrl, path]);
}

/** Owns file requests and unsaved drafts independently of tree/panel visibility. */
export function useWorkspaceEditor(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  refresh: () => void,
  onOpenPath: (path: string) => void,
  onLocatePath: (path: string, isDirectory: boolean) => void,
) {
  const [file, setFile] = useState<WorkspaceFile | null>(null);
  const [navigation, setNavigation] = useState<WorkspaceFileNavigation | null>(null);
  const [draft, updateDraft] = useState("");
  const [selectedPath, setSelectedPath] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState("");
  const [pathLoading, setPathLoading] = useState(false);
  const [pathFailure, setPathFailure] = useState<{ message: string; missingPath: string } | null>(
    null,
  );
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [reloadVersion, setReloadVersion] = useState(0);
  const entryChange = useRef(false);
  const [changingEntries, setChangingEntries] = useState(false);
  const fileRequest = useRef(0);
  const groupGeneration = useRef(0);
  const currentGroup = useRef(groupId);
  const currentScope = useRef({ scopeKey, scopeUrl });
  const drafts = useRef(new Map<string, { file: WorkspaceFile; draft: string }>());
  // Other open files can finish saving while a different file is on screen. Notify
  // consumers which show per-file status even when the visible draft did not change.
  const [, setEditRevision] = useState(0);
  const dirtyOwner = useRef(Symbol("workspace editor"));
  const publishDirty = useCallback(() => {
    setWorkspaceDirty(dirtyOwner.current, drafts.current.size > 0 || pendingSaves.current.size > 0);
    setEditRevision((revision) => revision + 1);
  }, []);
  // A save belongs to its file even when the user opens a different viewer.
  const pendingSaves = useRef(new Set<string>());
  const visibleFile = useRef(file);
  useLayoutEffect(() => {
    visibleFile.current = file;
  }, [file]);
  useLayoutEffect(() => {
    fileRequest.current += 1;
    // Another participant can change the active scope without navigating this page. Retire
    // reads and a pending navigation from the old scope, but keep its drafts and pending writes.
    if (
      currentGroup.current !== groupId ||
      currentScope.current.scopeKey !== scopeKey ||
      currentScope.current.scopeUrl !== scopeUrl
    )
      finishWorkspaceNavigation(false);
    if (currentGroup.current !== groupId) {
      groupGeneration.current += 1;
      currentGroup.current = groupId;
      drafts.current.clear();
      pendingSaves.current.clear();
    }
    currentScope.current = { scopeKey, scopeUrl };
    entryChange.current = false;
    setChangingEntries(false);
    setWorkspaceDirty(dirtyOwner.current, drafts.current.size > 0 || pendingSaves.current.size > 0);
    setFile(null);
    setNavigation(null);
    updateDraft("");
    setSelectedPath("");
    setFileError("");
    setFileLoading(false);
    setPathLoading(false);
    setPathFailure(null);
    setConflict(false);
    setSaving(false);
    setReloadVersion(0);
  }, [groupId, scopeKey, scopeUrl]);
  useLayoutEffect(() => {
    const owner = dirtyOwner.current;
    return () => {
      fileRequest.current += 1;
      groupGeneration.current += 1;
      setWorkspaceDirty(owner, false);
    };
  }, []);
  const setDraft = useCallback(
    (value: string) => {
      if (
        currentGroup.current !== groupId ||
        (file &&
          (file.scope_key !== currentScope.current.scopeKey ||
            file.scope_url !== currentScope.current.scopeUrl))
      )
        return;
      updateDraft(value);
      if (file) {
        const key = scopeFileKey(file.scope_key, file.scope_url, file.path);
        // The old disk content is still an edit if a pending save is replacing it.
        if (value === file.content && !pendingSaves.current.has(key)) drafts.current.delete(key);
        else drafts.current.set(key, { file, draft: value });
        publishDirty();
      }
    },
    [file, groupId, publishDirty],
  );

  const openFile = useCallback(
    async (path: string, options?: WorkspaceOpenFileOptions) => {
      const key = scopeFileKey(scopeKey, scopeUrl, path);
      if (entryChange.current || (options?.reload && pendingSaves.current.has(key))) return null;
      setPathLoading(false);
      setPathFailure(null);
      const destination = options?.fragment ? { fragment: options.fragment } : null;
      if (
        !options?.reload &&
        file?.path === path &&
        file.scope_key === scopeKey &&
        file.scope_url === scopeUrl
      ) {
        if (pathLoading || selectedPath !== path || destination) {
          fileRequest.current += 1;
          setSelectedPath(path);
          setFileLoading(false);
          setFileError("");
        }
        if (destination) setNavigation(destination);
        return path;
      }
      const request = ++fileRequest.current;
      setNavigation(null);
      setFileLoading(true);
      setFileError("");
      setConflict(false);
      setSaving(false);
      setSelectedPath(path);
      onOpenPath(path);
      const cached = drafts.current.get(key);
      if (cached && !options?.reload) {
        setFile(cached.file);
        setNavigation(destination);
        updateDraft(cached.draft);
        setSaving(pendingSaves.current.has(scopeFileKey(scopeKey, scopeUrl, cached.file.path)));
        setFileLoading(false);
        return cached.file.path;
      }
      const response = await api.fetchWorkspaceFile(groupId, path, scopeKey, scopeUrl);
      if (request !== fileRequest.current) return null;
      setFileLoading(false);
      if (!response.ok) {
        setFileError(response.error.message);
        return null;
      }
      // The server resolves internal symlinks to their canonical workspace path.
      // Look up that identity before replacing an unsaved target with disk bytes.
      const resolvedKey = scopeFileKey(scopeKey, scopeUrl, response.result.path);
      const latestDraft = drafts.current.get(resolvedKey);
      // A reload may discard only the draft that existed when it was requested.
      // Typing or a late save during the read must not be overwritten by disk bytes.
      const targetDraft =
        latestDraft && (!options?.reload || latestDraft !== cached) ? latestDraft : undefined;
      if (!targetDraft) drafts.current.delete(resolvedKey);
      publishDirty();
      setFile(targetDraft ? targetDraft.file : response.result);
      setSelectedPath(response.result.path);
      onOpenPath(response.result.path);
      setNavigation(destination);
      updateDraft(targetDraft ? targetDraft.draft : response.result.content);
      setSaving(pendingSaves.current.has(resolvedKey));
      if (options?.reload) setReloadVersion((version) => version + 1);
      return response.result.path;
    },
    [groupId, scopeKey, scopeUrl, file, selectedPath, pathLoading, onOpenPath, publishDirty],
  );

  /** Resolves a path and opens it when it is a file. Returns the opened file's path, else null. */
  const locatePath = useCallback(
    async (path: string): Promise<string | null> => {
      if (entryChange.current) return null;
      // Share the editor's navigation generation: a newer file click or scope change
      // must retire this lookup before it can open a file or move focus in the tree.
      const request = ++fileRequest.current;
      setFileLoading(false);
      setPathLoading(true);
      setPathFailure(null);
      setSelectedPath(file?.path ?? "");
      const response = await api.resolveWorkspacePath(groupId, path, scopeKey, scopeUrl);
      if (request !== fileRequest.current) return null;
      setPathLoading(false);
      if (!response.ok) {
        setPathFailure({
          message: response.error.message,
          missingPath: response.error.code === "NOT_FOUND" ? path : "",
        });
        return null;
      }
      if (response.result.is_dir) {
        onLocatePath(response.result.path, true);
        return null;
      }
      const opened = await openFile(response.result.path);
      if (opened !== null) onLocatePath(opened, false);
      return opened;
    },
    [groupId, scopeKey, scopeUrl, file, openFile, onLocatePath],
  );

  const closeFile = useCallback(() => {
    fileRequest.current += 1;
    setPathLoading(false);
    setPathFailure(null);
    setFile(null);
    setNavigation(null);
    updateDraft("");
    setFileError("");
    setFileLoading(false);
    setConflict(false);
    setSaving(false);
  }, []);

  const saveFile = useCallback(
    async (content: string) => {
      const key = file ? scopeFileKey(file.scope_key, file.scope_url, file.path) : "";
      if (
        !file ||
        entryChange.current ||
        file.scope_key !== scopeKey ||
        file.scope_url !== scopeUrl ||
        pendingSaves.current.has(key)
      )
        return false;
      const generation = groupGeneration.current;
      pendingSaves.current.add(key);
      publishDirty();
      setSaving(true);
      setFileError("");
      const response = await api.saveWorkspaceFile(
        groupId,
        file.path,
        content,
        file.sha256,
        file.scope_key,
        file.scope_url,
      );
      if (generation !== groupGeneration.current) return false;
      pendingSaves.current.delete(key);
      const cached = drafts.current.get(key);
      if (response.ok && cached && cached.file.sha256 === file.sha256) {
        if (cached.draft === content) drafts.current.delete(key);
        else
          drafts.current.set(key, {
            file: { ...cached.file, content, sha256: response.result.sha256 },
            draft: cached.draft,
          });
      }
      publishDirty();
      const isCurrentScope =
        currentScope.current.scopeKey === file.scope_key &&
        currentScope.current.scopeUrl === file.scope_url;
      if (isCurrentScope && visibleFile.current?.path === file.path) {
        setSaving(false);
        if (response.ok) {
          // Returning to the same file during a save must adopt its new digest, but
          // must not replace a newer baseline obtained by an explicit reload.
          setFile((current) =>
            current?.path === file.path && current.sha256 === file.sha256
              ? { ...current, content, sha256: response.result.sha256 }
              : current,
          );
        }
      }
      if (response.ok && isCurrentScope) refresh();
      // Directory lookups do not replace the editor. Report saves against the still-visible
      // file and baseline, including when the user leaves and returns while a save is pending.
      if (
        !isCurrentScope ||
        visibleFile.current?.path !== file.path ||
        visibleFile.current.sha256 !== file.sha256
      )
        return false;
      if (!response.ok) {
        setConflict(response.error.code === "workspace_write_conflict");
        setFileError(response.error.message);
        return false;
      }
      setConflict(false);
      return true;
    },
    [file, groupId, scopeKey, scopeUrl, refresh, publishDirty],
  );

  const beginEntryChange = useCallback(() => {
    if (entryChange.current || pendingSaves.current.size) return false;
    entryChange.current = true;
    setChangingEntries(true);
    fileRequest.current += 1;
    setFileLoading(false);
    setPathLoading(false);
    return true;
  }, []);
  const endEntryChange = useCallback(() => {
    entryChange.current = false;
    setChangingEntries(false);
  }, []);
  const hasDraftsUnder = useCallback(
    (path: string) =>
      [...drafts.current.values()].some(
        ({ file }) =>
          file.scope_key === scopeKey &&
          file.scope_url === scopeUrl &&
          (!path || file.path === path || file.path.startsWith(path + "/")),
      ),
    [scopeKey, scopeUrl],
  );
  const getFileEditStatus = useCallback(
    (path: string, targetScopeKey = scopeKey, targetScopeUrl = scopeUrl) => {
      const key = scopeFileKey(targetScopeKey, targetScopeUrl, path);
      return { dirty: drafts.current.has(key), saving: pendingSaves.current.has(key) };
    },
    [scopeKey, scopeUrl],
  );
  const getFileDraft = useCallback(
    (path: string, targetScopeKey = scopeKey, targetScopeUrl = scopeUrl) =>
      drafts.current.get(scopeFileKey(targetScopeKey, targetScopeUrl, path))?.draft ?? null,
    [scopeKey, scopeUrl],
  );
  const discardFileDraft = useCallback(
    (path: string, targetScopeKey = scopeKey, targetScopeUrl = scopeUrl) => {
      // A write already in flight could still change the baseline. Do not tell the
      // caller it is safe to close or discard until it has settled.
      const key = scopeFileKey(targetScopeKey, targetScopeUrl, path);
      if (entryChange.current || pendingSaves.current.has(key)) return false;
      if (!drafts.current.delete(key)) return true;
      const current = visibleFile.current;
      if (
        current?.path === path &&
        current.scope_key === targetScopeKey &&
        current.scope_url === targetScopeUrl
      )
        updateDraft(current.content);
      publishDirty();
      return true;
    },
    [scopeKey, scopeUrl, publishDirty],
  );
  const reconcileEntry = useCallback(
    (path: string, destination?: string, mimeType?: string) => {
      const remap = (value: string) =>
        value === path || value.startsWith(path + "/")
          ? destination === undefined
            ? null
            : destination + value.slice(path.length)
          : value;
      const movedFile = (file: WorkspaceFile, moved: string) => ({
        ...file,
        path: moved,
        mime_type: file.path === path && mimeType !== undefined ? mimeType : file.mime_type,
      });
      const next = new Map<string, { file: WorkspaceFile; draft: string }>();
      for (const [key, value] of drafts.current) {
        if (value.file.scope_key !== scopeKey || value.file.scope_url !== scopeUrl) {
          next.set(key, value);
          continue;
        }
        const moved = remap(value.file.path);
        if (moved !== null)
          next.set(scopeFileKey(scopeKey, scopeUrl, moved), {
            ...value,
            file: movedFile(value.file, moved),
          });
      }
      drafts.current = next;
      const current = visibleFile.current;
      if (current) {
        const moved = remap(current.path);
        if (moved === null) closeFile();
        else if (moved !== current.path) {
          const updated = movedFile(current, moved);
          visibleFile.current = updated;
          setFile(updated);
          setSelectedPath(moved);
          setNavigation(null);
        }
      }
      publishDirty();
    },
    [scopeKey, scopeUrl, closeFile, publishDirty],
  );

  return {
    beginEntryChange,
    endEntryChange,
    reconcileEntry,
    hasDraftsUnder,
    getFileEditStatus,
    getFileDraft,
    discardFileDraft,
    changingEntries,
    file,
    navigation,
    draft,
    setDraft,
    selectedPath,
    fileLoading,
    fileError,
    pathLoading,
    pathError: pathFailure?.message ?? "",
    missingPath: pathFailure?.missingPath ?? "",
    locatePath,
    saving,
    conflict,
    reloadVersion,
    openFile,
    closeFile,
    saveFile,
  };
}
