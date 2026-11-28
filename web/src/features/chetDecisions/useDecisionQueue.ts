import { useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { PendingSummaryContext } from "../pendingReplies/context";
import { checkedPage } from "../pendingReplies/usePendingPages";
import { fetchPendingReplies } from "../../services/api/pendingReplies";
import { useGroupStore } from "../../stores";
import type { LedgerEvent } from "../../types";
import { buildDecisionQueue, type DecisionGroupPage } from "./decisionQueueModel";

/** Groups read at once, so a long Group list does not open dozens of requests together. */
const CONCURRENCY = 4;

type State = {
  pages: DecisionGroupPage[];
  /** Groups whose first page holds only part of their open questions. */
  partial: string[];
  /** Groups whose questions could not be read. */
  failed: string[];
  loaded: boolean;
  loading: boolean;
};

const EMPTY: State = { pages: [], partial: [], failed: [], loaded: false, loading: false };

/**
 * The open questions of every Group, read through each Group's own pending list. The
 * list is read when the queue opens and on refresh; an answered question keeps its place
 * until then, showing its answer, so nothing jumps while the user works down the queue.
 */
export function useDecisionQueue(active: boolean) {
  const summary = useContext(PendingSummaryContext);
  const groups = useGroupStore((state) => state.groups);
  const titles = useMemo(
    () => new Map(groups.map((group) => [group.group_id, group.title || group.group_id])),
    [groups],
  );
  const [state, setState] = useState<State>(EMPTY);
  const visit = useRef(0);
  const summaries = summary?.summaries;
  const summariesRef = useRef(summaries);
  summariesRef.current = summaries;
  const titlesRef = useRef(titles);
  titlesRef.current = titles;

  const load = useCallback(() => {
    const request = ++visit.current;
    const known = summariesRef.current ?? {};
    // A Group with a known zero is skipped; an unknown count is read, as it may hide some.
    const groupIds = Object.keys(known).filter((id) => {
      const entry = known[id];
      return !(entry?.state === "fresh" && entry.pending === 0);
    });
    setState((current) => ({ ...current, loading: true }));
    void (async () => {
      const pages: DecisionGroupPage[] = [];
      const partial: string[] = [];
      const failed: string[] = [];
      let next = 0;
      const worker = async () => {
        while (next < groupIds.length) {
          const groupId = groupIds[next++]!;
          const response = await fetchPendingReplies(groupId).catch(() => null);
          if (request !== visit.current) return;
          let events: LedgerEvent[] | null = null;
          if (response?.ok) {
            try {
              events = checkedPage(response.result, groupId);
            } catch {
              events = null;
            }
          }
          if (!events) {
            failed.push(groupId);
            continue;
          }
          if (response?.ok && response.result.has_more) partial.push(groupId);
          if (events.length)
            pages.push({ groupId, groupTitle: titlesRef.current.get(groupId) ?? groupId, events });
        }
      };
      await Promise.all(Array.from({ length: Math.min(CONCURRENCY, groupIds.length) }, worker));
      if (request !== visit.current) return;
      setState({ pages, partial, failed, loaded: true, loading: false });
    })();
  }, []);

  const hasSummaries = !!summaries && Object.keys(summaries).length > 0;
  useEffect(() => {
    if (!active) {
      visit.current += 1;
      setState(EMPTY);
      return;
    }
    if (hasSummaries) load();
  }, [active, hasSummaries, load]);

  const [now] = useState(() => Date.now());
  const items = useMemo(() => buildDecisionQueue(state.pages, now), [now, state.pages]);
  return {
    available: !!summary,
    items,
    partial: state.partial,
    failed: state.failed,
    loaded: state.loaded,
    loading: state.loading || (active && !state.loaded && hasSummaries),
    refresh: () => {
      summary?.refresh();
      load();
    },
  };
}
