import { useEffect, useState } from "react";
import { apiJson } from "../../services/api/base";
import { directRelationClosed, type DirectStatus } from "./directConnectionModel";

/** Keep closed records out of the panel using the daemon's guarded removal API. */
export function useDirectStatus(groupId: string, revision: number) {
  const [status, setStatus] = useState<DirectStatus | null>(null);
  const [pollFailed, setPollFailed] = useState(false);
  useEffect(() => {
    let disposed = false;
    let controller: AbortController | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    // Retry failed cleanup on the next visit or explicit action, without creating
    // a write loop. Closed records never return to the visible list.
    const failures = new Set<string>();
    const poll = async () => {
      const request = new AbortController();
      controller = request;
      const deadline = setTimeout(() => request.abort(), 10000);
      const result = await apiJson<DirectStatus>(
        `/api/v1/connect/direct?group_id=${encodeURIComponent(groupId)}`,
        { signal: request.signal },
      );
      clearTimeout(deadline);
      if (disposed) return;
      setPollFailed(!result.ok);
      if (result.ok) {
        const closed = result.result.relations.filter(directRelationClosed);
        setStatus({ ...result.result });
        for (const relation of closed) {
          if (failures.has(relation.id)) continue;
          const cleanupDeadline = setTimeout(() => request.abort(), 10000);
          const removal = await apiJson("/api/v1/connect/direct", {
            method: "POST",
            signal: request.signal,
            body: JSON.stringify({ action: "remove", group_id: groupId, id: relation.id }),
          });
          clearTimeout(cleanupDeadline);
          if (disposed) return;
          if (!removal.ok) failures.add(relation.id);
        }
      } else {
        setStatus((previous) =>
          previous
            ? {
                ...previous,
                runtime: null,
                relations: previous.relations.map((r) => ({ ...r, online: false })),
              }
            : null,
        );
      }
      timer = setTimeout(() => void poll(), 3000);
    };
    void poll();
    return () => {
      disposed = true;
      controller?.abort();
      clearTimeout(timer);
    };
  }, [groupId, revision]);
  return { status, setStatus, pollFailed };
}
