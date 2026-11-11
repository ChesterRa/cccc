import { useCallback, useEffect, useRef, useState } from "react";
import {
  backlogOperation,
  fetchBacklog,
  type ApiResponse,
  type BacklogOperation,
} from "../../../services/api";
import type { BacklogSnapshot, BacklogWriteResult } from "../../../types";

export type BacklogScope = { scopeKey: string; scopeUrl: string };

export type BacklogLoadError = { code: string; message: string; details?: unknown };

export type BacklogState = {
  snapshot: BacklogSnapshot | null;
  loading: boolean;
  error: BacklogLoadError | null;
  reload: () => Promise<BacklogSnapshot | null>;
  run: (
    operation: BacklogOperation,
    body: Record<string, unknown>,
  ) => Promise<ApiResponse<BacklogWriteResult>>;
};

function snapshotOf(result: BacklogWriteResult): BacklogSnapshot {
  return {
    configured: result.configured,
    scope_key: result.scope_key,
    scope_url: result.scope_url,
    path: result.path,
    exists: result.exists,
    sha256: result.sha256,
    git: result.git,
    problems: result.problems,
    sections: result.sections,
  };
}

/**
 * The backlog of the scope the page shows. It is read when someone asks for it, never on a
 * timer, and a write's answer replaces it; an answer older than the latest request is dropped.
 */
export function useBacklog(groupId: string, scope: BacklogScope | null): BacklogState {
  const scopeKey = scope?.scopeKey || "";
  const scopeUrl = scope?.scopeUrl || "";
  const [snapshot, setSnapshot] = useState<BacklogSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<BacklogLoadError | null>(null);
  const sequence = useRef(0);

  // Another Group or project folder starts empty: its file is a different one.
  useEffect(() => {
    sequence.current += 1;
    setSnapshot(null);
    setError(null);
    setLoading(false);
  }, [groupId, scopeKey, scopeUrl]);

  const reload = useCallback(async (): Promise<BacklogSnapshot | null> => {
    if (!groupId || !scopeKey) return null;
    const request = ++sequence.current;
    setLoading(true);
    const response = await fetchBacklog(groupId, scopeKey, scopeUrl);
    if (sequence.current !== request) return null;
    setLoading(false);
    if (!response.ok) {
      setError(response.error);
      return null;
    }
    setError(null);
    setSnapshot(response.result);
    return response.result;
  }, [groupId, scopeKey, scopeUrl]);

  // A draft names the folder it was read from; the daemon refuses it once that folder changed.
  const run = useCallback(
    async (operation: BacklogOperation, body: Record<string, unknown>) => {
      const request = ++sequence.current;
      const response = await backlogOperation(groupId, operation, {
        scope_key: scopeKey,
        scope_url: scopeUrl,
        ...body,
      });
      if (sequence.current === request) {
        setLoading(false);
        if (response.ok) {
          setError(null);
          setSnapshot(snapshotOf(response.result));
        }
      }
      return response;
    },
    [groupId, scopeKey, scopeUrl],
  );

  return { snapshot, loading, error, reload, run };
}
