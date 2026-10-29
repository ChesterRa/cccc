import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  fetchPendingReplySummary,
  type PendingReplySummary,
} from "../../services/api/pendingReplies";

export type Summary = {
  state: "fresh" | "stale" | "unknown";
  pending: number | null;
  revision?: string;
  oldestTs?: string | null;
  asOf?: string;
  accessDenied?: boolean;
  /** The running daemon has no such operation, as one from another build may not. */
  daemonUnsupported?: boolean;
};

const UNKNOWN: Summary = { state: "unknown", pending: null };
const INTERVAL_MS = 5000;

function validSummary(value: PendingReplySummary): Summary {
  if (
    value.state !== "fresh" ||
    !Number.isSafeInteger(value.pending) ||
    (value.pending as number) < 0 ||
    typeof value.revision !== "string" ||
    !value.revision
  )
    return UNKNOWN;
  return {
    state: "fresh",
    pending: value.pending,
    revision: value.revision,
    oldestTs: value.oldest_ts,
    asOf: value.as_of,
  };
}

/** A single user-scoped read supplies all sidebar counts; failures are never zero. */
export function usePendingSummary(enabled: boolean, groupIds: readonly string[]) {
  const key = JSON.stringify([...new Set(groupIds.filter(Boolean))].sort());
  const ids = useMemo<string[]>(() => JSON.parse(key), [key]);
  const [entries, setEntries] = useState<Record<string, Summary>>({});
  const trigger = useRef<() => void>(() => {});
  const refresh = useCallback(() => trigger.current(), []);

  useEffect(() => {
    if (!enabled) {
      setEntries({});
      trigger.current = () => {};
      return;
    }
    let closed = false;
    let pending = false;
    let again = false;
    const controller = new AbortController();
    const allowed = new Set(ids);
    setEntries((current) => Object.fromEntries(ids.map((id) => [id, current[id] ?? UNKNOWN])));
    const read = async () => {
      if (!enabled || !ids.length || closed) return;
      if (pending) {
        again = true;
        return;
      }
      pending = true;
      const response = await fetchPendingReplySummary({ signal: controller.signal }).catch(
        () => null,
      );
      if (!closed) {
        if (response?.ok && response.result && Array.isArray(response.result.groups)) {
          const found = new Map<string, Summary>();
          const duplicates = new Set<string>();
          for (const group of response.result.groups) {
            if (!group || typeof group.group_id !== "string" || !allowed.has(group.group_id))
              continue;
            if (found.has(group.group_id)) duplicates.add(group.group_id);
            else found.set(group.group_id, validSummary(group));
          }
          setEntries(
            Object.fromEntries(
              ids.map((id) => [id, duplicates.has(id) ? UNKNOWN : (found.get(id) ?? UNKNOWN)]),
            ),
          );
        } else {
          const code =
            response && !response.ok ? String(response.error?.code || "").toLowerCase() : "";
          const denied = [
            "auth_required",
            "unauthorized",
            "permission_denied",
            "forbidden",
            "read_only",
          ].includes(code);
          const daemonUnsupported = code === "unknown_op";
          setEntries((current) =>
            Object.fromEntries(
              ids.map((id) => {
                const previous = current[id];
                return [
                  id,
                  denied
                    ? { ...UNKNOWN, accessDenied: true }
                    : daemonUnsupported
                      ? { ...UNKNOWN, daemonUnsupported: true }
                      : previous?.accessDenied
                        ? previous
                        : previous?.pending != null
                          ? { ...previous, state: "stale" as const }
                          : UNKNOWN,
                ];
              }),
            ),
          );
        }
      }
      pending = false;
      if (again && !closed) {
        again = false;
        void read();
      }
    };
    const visibleRead = () => {
      if (document.visibilityState !== "hidden") void read();
    };
    trigger.current = () => void read();
    visibleRead();
    const timer = window.setInterval(visibleRead, INTERVAL_MS);
    document.addEventListener("visibilitychange", visibleRead);
    return () => {
      closed = true;
      controller.abort();
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", visibleRead);
      trigger.current = () => {};
    };
  }, [enabled, ids]);

  // A scope change is applied immediately, even before the effect runs.
  const summaries = useMemo(
    () => (enabled ? Object.fromEntries(ids.map((id) => [id, entries[id] ?? UNKNOWN])) : {}),
    [enabled, entries, ids],
  );
  return { summaries, refresh };
}
