import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { useFormStore } from "../../stores";
import { directoryNameFromPath } from "./createGroupDirectoryModel";

export function useCreateGroupDirectoryBrowser() {
  const { t } = useTranslation("modals");
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const setDirItems = useFormStore((state) => state.setDirItems);
  const setCurrentDir = useFormStore((state) => state.setCurrentDir);
  const setParentDir = useFormStore((state) => state.setParentDir);
  const setShowDirBrowser = useFormStore((state) => state.setShowDirBrowser);
  const setCreateGroupPath = useFormStore((state) => state.setCreateGroupPath);
  const setCreateGroupName = useFormStore((state) => state.setCreateGroupName);

  const fetchContents = useCallback(
    async (path: string) => {
      setShowDirBrowser(true);
      setError("");
      const response = await api.fetchDirContents(path);
      if (!response.ok) {
        setError(response.error?.message || t("createGroup.failedToListDirectory"));
        return false;
      }
      setDirItems(response.result.items || []);
      setCurrentDir(response.result.path || path);
      setParentDir(response.result.parent || null);
      return true;
    },
    [setCurrentDir, setDirItems, setParentDir, setShowDirBrowser, t],
  );

  const createDirectory = useCallback(
    async (parent: string, name: string) => {
      setCreating(true);
      setError("");
      try {
        const response = await api.createDirectory(parent, name);
        if (!response.ok) {
          setError(response.error?.message || t("createGroup.failedToCreateDirectory"));
          return false;
        }
        const path = response.result.path;
        setCreateGroupPath(path);
        setCreateGroupName(directoryNameFromPath(path));
        return await fetchContents(path);
      } finally {
        setCreating(false);
      }
    },
    [fetchContents, setCreateGroupName, setCreateGroupPath, t],
  );

  return { error, setError, creating, fetchContents, createDirectory };
}
