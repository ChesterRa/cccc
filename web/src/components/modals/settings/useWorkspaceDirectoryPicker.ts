import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type { DirItem, DirSuggestion } from "../../../types";

export function useWorkspaceDirectoryPicker() {
  const { t } = useTranslation("settings");
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<DirItem[]>([]);
  const [currentDir, setCurrentDir] = useState("");
  const [parentDir, setParentDir] = useState<string | null>(null);
  const [locations, setLocations] = useState<DirSuggestion[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const requestSeq = useRef(0);
  useEffect(
    () => () => {
      requestSeq.current += 1;
    },
    [],
  );

  const begin = () => {
    setBusy(true);
    setError("");
    return ++requestSeq.current;
  };
  const load = async (path: string, seq: number) => {
    try {
      const response = await api.fetchDirContents(path);
      if (seq !== requestSeq.current) return false;
      if (!response.ok) {
        setError(response.error?.message || t("copyGroups.failedToListWorkspace"));
        return false;
      }
      setItems(response.result.items);
      setCurrentDir(response.result.path);
      setParentDir(response.result.parent);
      setError("");
      return true;
    } catch {
      if (seq === requestSeq.current) setError(t("copyGroups.failedToListWorkspace"));
      return false;
    }
  };
  const finish = (seq: number) => {
    if (seq === requestSeq.current) setBusy(false);
  };
  const fetchDirectory = async (path: string) => {
    const seq = begin();
    try {
      return await load(path, seq);
    } finally {
      finish(seq);
    }
  };
  const show = async (path: string) => {
    const seq = begin();
    setOpen(true);
    setItems([]);
    setCurrentDir("");
    setParentDir(null);
    setLocations([]);
    try {
      const suggestions = await api.fetchDirSuggestions();
      if (seq !== requestSeq.current) return;
      if (suggestions.ok) setLocations(suggestions.result.suggestions);
      if (path && (await load(path, seq))) return;
      if (seq !== requestSeq.current) return;
      const fallback = suggestions.ok
        ? suggestions.result.suggestions.find((item) => item.icon === "desktop") ||
          suggestions.result.suggestions.find((item) => item.icon === "home")
        : undefined;
      if (fallback) await load(fallback.path, seq);
    } catch {
      if (seq === requestSeq.current) setError(t("copyGroups.failedToListWorkspace"));
    } finally {
      finish(seq);
    }
  };
  const createDirectory = async (parent: string, name: string) => {
    const seq = begin();
    try {
      const response = await api.createDirectory(parent, name);
      if (seq !== requestSeq.current) return false;
      if (!response.ok) {
        setError(response.error?.message || t("copyGroups.failedToCreateWorkspace"));
        return false;
      }
      return await load(response.result.path, seq);
    } catch {
      if (seq === requestSeq.current) setError(t("copyGroups.failedToCreateWorkspace"));
      return false;
    } finally {
      finish(seq);
    }
  };
  const close = () => {
    requestSeq.current += 1;
    setOpen(false);
    setBusy(false);
  };
  return {
    open,
    items,
    currentDir,
    parentDir,
    locations,
    error,
    busy,
    show,
    close,
    fetchDirectory,
    createDirectory,
  };
}
