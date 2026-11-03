import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { useFormStore } from "../../stores";
import { directoryCreationErrorKey, directoryNameFromPath } from "./createGroupDirectoryModel";

export function useCreateGroupDirectoryBrowser() {
  const { t } = useTranslation("modals");
  const [error, setError] = useState("");
  const [createError, setCreateError] = useState("");
  const [retryPath, setRetryPath] = useState("");
  const [creating, setCreating] = useState(false);
  const requestSeq = useRef(0);
  useEffect(
    () => () => {
      requestSeq.current += 1;
    },
    [],
  );
  const setDirItems = useFormStore((state) => state.setDirItems);
  const setCurrentDir = useFormStore((state) => state.setCurrentDir);
  const setParentDir = useFormStore((state) => state.setParentDir);
  const setShowDirBrowser = useFormStore((state) => state.setShowDirBrowser);
  const setCreateGroupPath = useFormStore((state) => state.setCreateGroupPath);
  const setCreateGroupName = useFormStore((state) => state.setCreateGroupName);

  const fetchContents = useCallback(
    async (path: string) => {
      const seq = ++requestSeq.current;
      setShowDirBrowser(true);
      setError("");
      setCreateError("");
      setRetryPath("");
      try {
        const response = await api.fetchDirContents(path);
        if (seq !== requestSeq.current) return false;
        if (!response.ok) {
          setError(response.error?.message || t("createGroup.failedToListDirectory"));
          return false;
        }
        setDirItems(response.result.items || []);
        setCurrentDir(response.result.path || path);
        setParentDir(response.result.parent || null);
        return true;
      } catch {
        if (seq === requestSeq.current) setError(t("createGroup.failedToListDirectory"));
        return false;
      }
    },
    [setCurrentDir, setDirItems, setParentDir, setShowDirBrowser, t],
  );

  const createDirectory = useCallback(
    async (parent: string, name: string) => {
      setCreating(true);
      setError("");
      setCreateError("");
      setRetryPath("");
      const seq = ++requestSeq.current;
      try {
        const response = await api.createDirectory(parent, name);
        if (seq !== requestSeq.current) return false;
        if (!response.ok) {
          const key = directoryCreationErrorKey(response.error?.code);
          setCreateError(
            key ? t(key) : response.error?.message || t("createGroup.failedToCreateDirectory"),
          );
          return false;
        }
        const path = response.result.path;
        setCreateGroupPath(path);
        setCreateGroupName(directoryNameFromPath(path));
        const loaded = await fetchContents(path);
        if (!loaded && requestSeq.current === seq + 1) {
          setError(t("createGroup.createdFolderUnreadable", { path }));
          setRetryPath(path);
        }
        return loaded;
      } catch {
        if (seq === requestSeq.current) setCreateError(t("createGroup.failedToCreateDirectory"));
        return false;
      } finally {
        setCreating(false);
      }
    },
    [fetchContents, setCreateGroupName, setCreateGroupPath, t],
  );

  const retryCreatedDirectory = async () => {
    const path = retryPath;
    if (!path) return false;
    const seq = requestSeq.current + 1;
    const loaded = await fetchContents(path);
    if (!loaded && requestSeq.current === seq) {
      setError(t("createGroup.createdFolderUnreadable", { path }));
      setRetryPath(path);
    }
    return loaded;
  };

  return {
    error,
    setError,
    createError,
    clearCreateError: () => setCreateError(""),
    retryPath,
    retryCreatedDirectory,
    creating,
    fetchContents,
    createDirectory,
  };
}
