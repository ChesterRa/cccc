import { useContext, useEffect, useMemo, useState } from "react";
import { fetchLedgerStatuses } from "../../services/api";
import type { ChatMessageData, LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { projectMessageMode } from "../../utils/crossGroupRecipients";
import { PendingSummaryContext } from "./context";

/** Batch the visible conversation's requests, including newly arrived SSE rows. */
export function useLivePendingStatuses(
  groupId: string,
  events: readonly LedgerEvent[],
  enabled: boolean,
) {
  const summary = useContext(PendingSummaryContext);
  const group = summary?.summaries[groupId];
  const idsKey = JSON.stringify(
    [
      ...new Set(
        events
          .filter((event) => {
            if (event.kind !== "chat.message" || event.by === "user" || !event.id) return false;
            const data = event.data as ChatMessageData | undefined;
            if (!data || projectMessageMode(data) !== "request_reply") return false;
            return (
              !!event._user_request ||
              (Array.isArray(data.to) && data.to.some((to) => to === "user" || to === "@user"))
            );
          })
          .map((event) => event.id!),
      ),
    ].sort(),
  );
  const ids = useMemo<string[]>(() => JSON.parse(idsKey), [idsKey]);
  const valid = enabled && !!summary && !!groupId && group?.state === "fresh" && !!group.revision;
  const key = valid ? JSON.stringify([groupId, group!.revision, ids]) : "";
  const [state, setState] = useState<{
    groupId: string;
    statuses: Record<string, LedgerEventStatusPayload>;
    verified: boolean;
  }>({ groupId: "", statuses: {}, verified: false });
  useEffect(() => {
    if (!valid || !ids.length) return;
    const controller = new AbortController();
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const read = async () => {
      const statuses: Record<string, LedgerEventStatusPayload> = {};
      try {
        for (let start = 0; start < ids.length; start += 1000) {
          const response = await fetchLedgerStatuses(groupId, ids.slice(start, start + 1000), {
            noCache: true,
            signal: controller.signal,
          });
          if (!response.ok || !response.result?.statuses) throw new Error("status unavailable");
          Object.assign(statuses, response.result.statuses);
        }
        if (!closed) setState({ groupId, statuses, verified: true });
      } catch {
        if (!closed) {
          setState((current) => ({
            groupId,
            statuses: current.groupId === groupId ? current.statuses : {},
            verified: false,
          }));
          retry = setTimeout(() => void read(), 5000);
        }
      }
    };
    void read();
    return () => {
      closed = true;
      if (retry !== undefined) clearTimeout(retry);
      controller.abort();
    };
  }, [valid, groupId, ids, key]);
  return useMemo(
    () =>
      Object.fromEntries(
        ids.map((id) => [
          id,
          {
            status: state.groupId === groupId ? (state.statuses[id] ?? null) : null,
            // A verified row remains usable while a new revision is read.
            // The daemon rechecks it before accepting an answer. New rows,
            // another Group, and failed/stale reads remain unavailable.
            checking: !valid || !state.verified || state.groupId !== groupId || !state.statuses[id],
          },
        ]),
      ),
    [groupId, ids, state, valid],
  );
}
