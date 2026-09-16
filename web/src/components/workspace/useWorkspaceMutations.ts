import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  resolveWorkspacePath,
  changeWorkspaceEntry,
  uploadWorkspaceFile,
  type WorkspaceOperation,
} from "../../services/api/workspace";
import { setWorkspaceDirty } from "../../stores/workspaceNavigation";
import { joinWorkspacePath, type UploadEntry } from "./workspaceUploads";

type Editor = {
  beginEntryChange: () => boolean;
  endEntryChange: () => void;
  reconcileEntry: (path: string, destination?: string, mimeType?: string) => void;
  hasDraftsUnder: (path: string) => boolean;
};
export type UploadProgress = { completed: number; total: number; path: string; stopped: boolean };

export function useWorkspaceMutations(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  editor: Editor,
  refresh: () => void,
) {
  const { t } = useTranslation("chat");
  const { beginEntryChange, endEntryChange, reconcileEntry, hasDraftsUnder } = editor;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(null);
  const generation = useRef(0);
  const active = useRef(false);
  const controller = useRef<AbortController | null>(null);
  const owner = useRef(Symbol("workspace operation"));
  const retire = useCallback(() => {
    generation.current += 1;
    controller.current?.abort();
    setWorkspaceDirty(owner.current, false);
  }, []);
  useLayoutEffect(() => {
    generation.current++;
    active.current = false;
    setBusy(false);
    setError("");
    setUploadProgress(null);
    return retire;
  }, [groupId, scopeKey, scopeUrl, retire]);
  const begin = useCallback(() => {
    if (active.current) return false;
    if (!beginEntryChange()) {
      setError(t("workspaceManage.waitForSave"));
      return false;
    }
    active.current = true;
    setBusy(true);
    setError("");
    setWorkspaceDirty(owner.current, true);
    return true;
  }, [beginEntryChange, t]);
  const finish = useCallback(() => {
    active.current = false;
    setBusy(false);
    endEntryChange();
    setWorkspaceDirty(owner.current, false);
    // A failed recursive removal or interrupted batch may still have changed some entries.
    refresh();
  }, [endEntryChange, refresh]);
  const change = useCallback(
    async (operation: WorkspaceOperation) => {
      if (!begin()) return false;
      setUploadProgress(null);
      const token = generation.current;
      if (operation.operation === "move" && hasDraftsUnder("")) {
        const split = operation.destination.lastIndexOf("/");
        const parent = split < 0 ? "" : operation.destination.slice(0, split);
        const name = operation.destination.slice(split + 1);
        const resolved = await resolveWorkspacePath(groupId, parent, scopeKey, scopeUrl);
        if (token !== generation.current) return false;
        if (!resolved.ok) {
          setError(resolved.error.message);
          finish();
          return false;
        }
        const destination = joinWorkspacePath(resolved.result.path, name);
        if (
          destination !== operation.path &&
          !destination.startsWith(operation.path + "/") &&
          hasDraftsUnder(destination)
        ) {
          setError(t("workspaceManage.destinationDraft"));
          finish();
          return false;
        }
        operation = { ...operation, destination };
      }
      const response = await changeWorkspaceEntry(groupId, scopeKey, scopeUrl, operation);
      if (token !== generation.current) return false;
      if (response.ok) {
        if (operation.operation === "move")
          reconcileEntry(
            response.result.path,
            response.result.destination,
            response.result.mime_type,
          );
        if (operation.operation === "delete") reconcileEntry(response.result.path);
      } else
        setError(
          response.error.message +
            (operation.operation === "delete" ? ` ${t("workspaceManage.partialDelete")}` : ""),
        );
      finish();
      return response.ok;
    },
    [begin, finish, groupId, scopeKey, scopeUrl, reconcileEntry, hasDraftsUnder, t],
  );
  const upload = useCallback(
    async (directory: string, selection: Promise<UploadEntry[]> | UploadEntry[]) => {
      // Attach a rejection handler even when a busy operation declines a drop.
      const prepared = Promise.resolve(selection).then(
        (entries) => ({ entries }),
        (cause) => ({ cause }),
      );
      if (!begin()) return;
      const token = generation.current;
      const abort = new AbortController();
      controller.current = abort;
      let completed = 0;
      setUploadProgress({ completed, total: 0, path: directory, stopped: false });
      try {
        const result = await prepared;
        if ("cause" in result) throw result.cause;
        if (token !== generation.current) return;
        const entries = result.entries;
        for (const entry of entries) {
          if (abort.signal.aborted) break;
          const path = joinWorkspacePath(directory, entry.path);
          setUploadProgress({ completed, total: entries.length, path, stopped: false });
          const response = entry.file
            ? await uploadWorkspaceFile(groupId, scopeKey, scopeUrl, path, entry.file, abort.signal)
            : await changeWorkspaceEntry(groupId, scopeKey, scopeUrl, {
                operation: "create",
                path,
                directory: true,
              });
          if (token !== generation.current) return;
          if (!response.ok) {
            if (!abort.signal.aborted) setError(`${path}: ${response.error.message}`);
            break;
          }
          completed++;
        }
        if (token === generation.current)
          setUploadProgress({ completed, total: entries.length, path: directory, stopped: true });
      } catch (cause) {
        if (token === generation.current) {
          const code = cause instanceof Error ? cause.message : "unsupportedDrop";
          const known = ["invalidPath", "duplicatePath", "uploadLimit", "unsupportedDrop"].includes(
            code,
          );
          setError(known ? t(`workspaceManage.${code}`) : t("workspaceManage.unreadableDrop"));
          setUploadProgress(null);
        }
      } finally {
        if (token === generation.current) {
          controller.current = null;
          finish();
        }
      }
    },
    [begin, finish, groupId, scopeKey, scopeUrl, t],
  );
  const cancelUpload = useCallback(() => controller.current?.abort(), []);
  return {
    busy,
    error,
    change,
    upload,
    uploadProgress,
    cancelUpload,
    clearFeedback: () => {
      setError("");
      setUploadProgress(null);
    },
  };
}
