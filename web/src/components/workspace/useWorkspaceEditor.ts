import { useCallback, useLayoutEffect, useRef, useState } from "react";
import * as api from "../../services/api";
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
) {
  const [file, setFile] = useState<WorkspaceFile | null>(null);
  const [navigation, setNavigation] = useState<WorkspaceFileNavigation | null>(null);
  const [draft, updateDraft] = useState("");
  const [selectedPath, setSelectedPath] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState("");
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const fileRequest = useRef(0);
  const groupGeneration = useRef(0);
  const drafts = useRef(new Map<string, { file: WorkspaceFile; draft: string }>());
  // A save belongs to its file even when the user opens a different viewer.
  const pendingSaves = useRef(new Set<string>());
  const visibleFile = useRef(file);
  useLayoutEffect(() => {
    visibleFile.current = file;
  }, [file]);
  useLayoutEffect(() => {
    fileRequest.current += 1;
    groupGeneration.current += 1;
    drafts.current.clear();
    pendingSaves.current.clear();
    setFile(null);
    setNavigation(null);
    updateDraft("");
    setSelectedPath("");
    setFileError("");
    setFileLoading(false);
    setConflict(false);
    setSaving(false);
  }, [groupId, scopeKey, scopeUrl]);
  const setDraft = useCallback(
    (value: string) => {
      updateDraft(value);
      if (file) {
        // The old disk content is still an edit if a pending save is replacing it.
        if (value === file.content && !pendingSaves.current.has(file.path))
          drafts.current.delete(file.path);
        else drafts.current.set(file.path, { file, draft: value });
      }
    },
    [file],
  );

  const openFile = useCallback(
    async (path: string, options?: WorkspaceOpenFileOptions) => {
      const destination = options?.fragment ? { fragment: options.fragment } : null;
      if (!options?.reload && file?.path === path) {
        if (selectedPath !== path || destination) {
          fileRequest.current += 1;
          setSelectedPath(path);
          setFileLoading(false);
          setFileError("");
        }
        if (destination) setNavigation(destination);
        return;
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
        return;
      }
      const response = await api.fetchWorkspaceFile(groupId, path, scopeKey, scopeUrl);
      if (request !== fileRequest.current) return;
      setFileLoading(false);
      if (!response.ok) {
        setFileError(response.error.message);
        return;
      }
      // The server resolves internal symlinks to their canonical workspace path.
      // Look up that identity before replacing an unsaved target with disk bytes.
      const targetDraft = !options?.reload && drafts.current.get(response.result.path);
      if (!targetDraft) drafts.current.delete(response.result.path);
      setFile(targetDraft ? targetDraft.file : response.result);
      setNavigation(destination);
      updateDraft(targetDraft ? targetDraft.draft : response.result.content);
      setSaving(pendingSaves.current.has(response.result.path));
    },
    [groupId, scopeKey, scopeUrl, file, selectedPath, onOpenPath],
  );

  const closeFile = useCallback(() => {
    fileRequest.current += 1;
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
        file.scope_key !== scopeKey ||
        file.scope_url !== scopeUrl ||
        pendingSaves.current.has(file.path)
      )
        return false;
      const request = fileRequest.current;
      const generation = groupGeneration.current;
      pendingSaves.current.add(file.path);
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
      // Errors belong to the request's viewer, not to a later navigation.
      if (request !== fileRequest.current) return false;
      if (!response.ok) {
        setConflict(response.error.code === "workspace_write_conflict");
        setFileError(response.error.message);
        return false;
      }
      setConflict(false);
      return true;
    },
    [file, groupId, scopeKey, scopeUrl, refresh],
  );

  return {
    file,
    navigation,
    draft,
    setDraft,
    selectedPath,
    fileLoading,
    fileError,
    saving,
    conflict,
    openFile,
    closeFile,
    saveFile,
  };
}
