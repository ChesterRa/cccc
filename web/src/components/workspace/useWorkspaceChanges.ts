import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import {
  fetchWorkspaceChanges,
  fetchWorkspaceDiff,
  type WorkspaceChanges,
  type WorkspacePatch,
  type WorkspaceDiffSide,
} from "../../services/api/workspace";

export function useWorkspaceChanges(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  active: boolean,
  revision: number,
) {
  const [status, setStatus] = useState<WorkspaceChanges | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [selection, setSelection] = useState<{ path: string; side: WorkspaceDiffSide } | null>(
    null,
  );
  const [patch, setPatch] = useState<WorkspacePatch | null>(null);
  const [patchError, setPatchError] = useState("");
  const [patchLoading, setPatchLoading] = useState(false);
  useLayoutEffect(() => {
    setStatus(null);
    setError("");
    setSelection(null);
    setPatch(null);
    setPatchError("");
  }, [groupId, scopeKey, scopeUrl]);
  useEffect(() => {
    if (!active || !groupId || !scopeKey || !scopeUrl) return;
    const abort = new AbortController();
    setLoading(true);
    setError("");
    setStatus(null);
    void fetchWorkspaceChanges(groupId, scopeKey, scopeUrl, abort.signal).then((response) => {
      if (abort.signal.aborted) return;
      setLoading(false);
      if (response.ok) setStatus(response.result);
      else setError(response.error.message);
    });
    return () => abort.abort();
  }, [groupId, scopeKey, scopeUrl, active, revision]);
  useEffect(() => {
    if (!selection || !active) return;
    const abort = new AbortController();
    setPatch(null);
    setPatchError("");
    setPatchLoading(true);
    void fetchWorkspaceDiff(
      groupId,
      scopeKey,
      scopeUrl,
      selection.path,
      selection.side,
      abort.signal,
    ).then((response) => {
      if (abort.signal.aborted) return;
      setPatchLoading(false);
      if (response.ok) setPatch(response.result);
      else setPatchError(response.error.message);
    });
    return () => abort.abort();
  }, [groupId, scopeKey, scopeUrl, active, selection, revision]);
  const open = useCallback(
    (path: string, side: WorkspaceDiffSide) => setSelection({ path, side }),
    [],
  );
  const close = useCallback(() => setSelection(null), []);
  return { status, error, loading, selection, patch, patchError, patchLoading, open, close };
}
