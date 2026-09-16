import { useCallback, useLayoutEffect, useRef, useState } from "react";
import * as api from "../../services/api";
import { setWorkspaceDirty } from "../../stores/workspaceNavigation";
import type { WorkspaceFile } from "../../types";

export type WorkspaceOpenFileOptions = { reload?: boolean; fragment?: string };
export type WorkspaceFileNavigation = { fragment: string };

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
  const [pathError, setPathError] = useState("");
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [reloadVersion, setReloadVersion] = useState(0);
  const entryChange = useRef(false);
  const [changingEntries, setChangingEntries] = useState(false);
  const fileRequest = useRef(0);
  const groupGeneration = useRef(0);
  const drafts = useRef(new Map<string, { file: WorkspaceFile; draft: string }>());
  const dirtyOwner = useRef(Symbol("workspace editor"));
  const publishDirty = useCallback(() => {
    setWorkspaceDirty(dirtyOwner.current, drafts.current.size > 0 || pendingSaves.current.size > 0);
  }, []);
  // A save belongs to its file even when the user opens a different viewer.
  const pendingSaves = useRef(new Set<string>());
  const visibleFile = useRef(file);
  useLayoutEffect(() => {
    visibleFile.current = file;
  }, [file]);
  useLayoutEffect(() => {
    fileRequest.current += 1;
    groupGeneration.current += 1;
    entryChange.current = false;
    setChangingEntries(false);
    drafts.current.clear();
    pendingSaves.current.clear();
    setWorkspaceDirty(dirtyOwner.current, false);
    setFile(null);
    setNavigation(null);
    updateDraft("");
    setSelectedPath("");
    setFileError("");
    setFileLoading(false);
    setPathLoading(false);
    setPathError("");
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
      updateDraft(value);
      if (file) {
        // The old disk content is still an edit if a pending save is replacing it.
        if (value === file.content && !pendingSaves.current.has(file.path))
          drafts.current.delete(file.path);
        else drafts.current.set(file.path, { file, draft: value });
        publishDirty();
      }
    },
    [file, publishDirty],
  );

  const openFile = useCallback(
    async (path: string, options?: WorkspaceOpenFileOptions) => {
      if (entryChange.current || (options?.reload && pendingSaves.current.has(path))) return null;
      setPathLoading(false);
      setPathError("");
      const destination = options?.fragment ? { fragment: options.fragment } : null;
      if (!options?.reload && file?.path === path) {
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
      const cached = drafts.current.get(path);
      if (cached && !options?.reload) {
        setFile(cached.file);
        setNavigation(destination);
        updateDraft(cached.draft);
        setSaving(pendingSaves.current.has(cached.file.path));
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
      const latestDraft = drafts.current.get(response.result.path);
      // A reload may discard only the draft that existed when it was requested.
      // Typing or a late save during the read must not be overwritten by disk bytes.
      const targetDraft =
        latestDraft && (!options?.reload || latestDraft !== cached) ? latestDraft : undefined;
      if (!targetDraft) drafts.current.delete(response.result.path);
      publishDirty();
      setFile(targetDraft ? targetDraft.file : response.result);
      setSelectedPath(response.result.path);
      onOpenPath(response.result.path);
      setNavigation(destination);
      updateDraft(targetDraft ? targetDraft.draft : response.result.content);
      setSaving(pendingSaves.current.has(response.result.path));
      if (options?.reload) setReloadVersion((version) => version + 1);
      return response.result.path;
    },
    [groupId, scopeKey, scopeUrl, file, selectedPath, pathLoading, onOpenPath, publishDirty],
  );

  const locatePath = useCallback(
    async (path: string) => {
      if (entryChange.current) return;
      // Share the editor's navigation generation: a newer file click or scope change
      // must retire this lookup before it can open a file or move focus in the tree.
      const request = ++fileRequest.current;
      setFileLoading(false);
      setPathLoading(true);
      setPathError("");
      setSelectedPath(file?.path ?? "");
      const response = await api.resolveWorkspacePath(groupId, path, scopeKey, scopeUrl);
      if (request !== fileRequest.current) return;
      setPathLoading(false);
      if (!response.ok) {
        setPathError(response.error.message);
        return;
      }
      if (response.result.is_dir) {
        onLocatePath(response.result.path, true);
      } else {
        const opened = await openFile(response.result.path);
        if (opened !== null) onLocatePath(opened, false);
      }
    },
    [groupId, scopeKey, scopeUrl, file, openFile, onLocatePath],
  );

  const closeFile = useCallback(() => {
    fileRequest.current += 1;
    setPathLoading(false);
    setPathError("");
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
      if (
        !file ||
        entryChange.current ||
        file.scope_key !== scopeKey ||
        file.scope_url !== scopeUrl ||
        pendingSaves.current.has(file.path)
      )
        return false;
      const generation = groupGeneration.current;
      pendingSaves.current.add(file.path);
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
      pendingSaves.current.delete(file.path);
      const cached = drafts.current.get(file.path);
      if (response.ok && cached && cached.file.sha256 === file.sha256) {
        if (cached.draft === content) drafts.current.delete(file.path);
        else
          drafts.current.set(file.path, {
            file: { ...cached.file, content, sha256: response.result.sha256 },
            draft: cached.draft,
          });
      }
      publishDirty();
      if (visibleFile.current?.path === file.path) {
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
      if (response.ok) refresh();
      // Directory lookups do not replace the editor. Report saves against the still-visible
      // file and baseline, including when the user leaves and returns while a save is pending.
      if (visibleFile.current?.path !== file.path || visibleFile.current.sha256 !== file.sha256)
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
      [...drafts.current.keys()].some((key) => !path || key === path || key.startsWith(path + "/")),
    [],
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
        const moved = remap(key);
        if (moved !== null) next.set(moved, { ...value, file: movedFile(value.file, moved) });
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
    [closeFile, publishDirty],
  );

  return {
    beginEntryChange,
    endEntryChange,
    reconcileEntry,
    hasDraftsUnder,
    changingEntries,
    file,
    navigation,
    draft,
    setDraft,
    selectedPath,
    fileLoading,
    fileError,
    pathLoading,
    pathError,
    locatePath,
    saving,
    conflict,
    reloadVersion,
    openFile,
    closeFile,
    saveFile,
  };
}
