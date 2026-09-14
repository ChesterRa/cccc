import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../../services/api";
import type { WorkspaceFile } from "../../types";

/** Owns file requests and unsaved drafts independently of tree/panel visibility. */
export function useWorkspaceEditor(
  groupId: string,
  refresh: () => void,
  onOpenPath: (path: string) => void,
) {
  const [file, setFile] = useState<WorkspaceFile | null>(null);
  // Owned here so hiding or remounting the viewer cannot discard unsaved edits.
  const [draft, updateDraft] = useState("");
  // Survives closing the viewer so the tree still shows which file was last open.
  const [selectedPath, setSelectedPath] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState("");
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const fileRequest = useRef(0);
  const groupGeneration = useRef(0);
  const drafts = useRef(new Map<string, { file: WorkspaceFile; draft: string }>());
  useEffect(() => {
    fileRequest.current += 1;
    groupGeneration.current += 1;
    drafts.current.clear();
    setFile(null);
    updateDraft("");
    setSelectedPath("");
    setFileError("");
    setFileLoading(false);
    setConflict(false);
    setSaving(false);
  }, [groupId]);
  const setDraft = useCallback(
    (value: string) => {
      updateDraft(value);
      if (file) {
        if (value === file.content) drafts.current.delete(file.path);
        else drafts.current.set(file.path, { file, draft: value });
      }
    },
    [file],
  );

  const openFile = useCallback(
    async (path: string, options?: { reload?: boolean }) => {
      if (!options?.reload && file?.path === path) {
        if (selectedPath !== path) {
          fileRequest.current += 1;
          setSelectedPath(path);
          setFileLoading(false);
          setFileError("");
        }
        return;
      }
      const request = ++fileRequest.current;
      setFileLoading(true);
      setFileError("");
      setConflict(false);
      setSaving(false);
      setSelectedPath(path);
      onOpenPath(path);
      const cached = drafts.current.get(path);
      if (cached && !options?.reload) {
        setFile(cached.file);
        updateDraft(cached.draft);
        setFileLoading(false);
        return;
      }
      const response = await api.fetchWorkspaceFile(groupId, path);
      // A group switch, a newer open, or a close makes this answer obsolete: applying it would
      // show one group's file while saves would target another. Each of those bumps
      // `fileRequest`, so that counter alone decides.
      if (request !== fileRequest.current) return;
      setFileLoading(false);
      if (!response.ok) {
        setFileError(response.error.message);
        return;
      }
      drafts.current.delete(path);
      setFile(response.result);
      updateDraft(response.result.content);
    },
    [groupId, file, selectedPath, onOpenPath],
  );

  const closeFile = useCallback(() => {
    // Retires any in-flight open so a late response cannot reopen the viewer.
    fileRequest.current += 1;
    setFile(null);
    updateDraft("");
    setFileError("");
    setFileLoading(false);
    setConflict(false);
    setSaving(false);
  }, []);

  const saveFile = useCallback(
    async (content: string) => {
      if (!file) return false;
      const request = fileRequest.current;
      const generation = groupGeneration.current;
      setSaving(true);
      setFileError("");
      const response = await api.saveWorkspaceFile(groupId, file.path, content, file.sha256);
      const cached = drafts.current.get(file.path);
      if (response.ok && generation === groupGeneration.current && cached) {
        if (cached.draft === content) drafts.current.delete(file.path);
        else
          drafts.current.set(file.path, {
            file: { ...cached.file, content, sha256: response.result.sha256 },
            draft: cached.draft,
          });
      }
      // The viewer may have moved on to another group or file while the write was in flight.
      // Whatever moved it bumped `fileRequest` and cleared `saving` for the new file, so this
      // stale answer must not touch either.
      if (request !== fileRequest.current) return false;
      setSaving(false);
      if (!response.ok) {
        // A conflict means an Actor touched the same file; the panel offers a reload.
        setConflict(response.error.code === "workspace_write_conflict");
        setFileError(response.error.message);
        return false;
      }
      setConflict(false);
      setFile((current) =>
        current && current.path === file.path
          ? { ...current, content, sha256: response.result.sha256 }
          : current,
      );
      // A new or newly dirty file changes its row's git badge.
      refresh();
      return true;
    },
    [file, groupId, refresh],
  );

  return {
    file,
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
