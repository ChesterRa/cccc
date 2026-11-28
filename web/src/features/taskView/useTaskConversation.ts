import { useCallback, useEffect, useRef, useState } from "react";
import { shouldShowInConversation } from "../../hooks/chat/chatTabBasics";
import { searchChatMessages } from "../../services/api";
import type { LedgerEvent } from "../../types";
import { isFormalChatMessageEvent } from "../../utils/chatSend";

const PAGE_SIZE = 200;
/** A task named in more messages than this shows its most recent ones, and says so. */
export const MAX_TASK_SEARCH_PAGES = 10;

type State = {
  key: string;
  events: LedgerEvent[];
  loaded: boolean;
  loading: boolean;
  error: boolean;
  truncated: boolean;
};

function empty(key: string): State {
  return { key, events: [], loaded: false, loading: false, error: false, truncated: false };
}

/**
 * Messages that reference or name the task, newest page first. The daemon applies
 * the task filter before counting results and advancing the history cursor.
 */
export function useTaskConversation(groupId: string, taskId: string) {
  const key = groupId && taskId ? JSON.stringify([groupId, taskId]) : "";
  const [state, setState] = useState<State>(() => empty(key));
  const visit = useRef<AbortController | null>(null);

  const load = useCallback(() => {
    if (!key) return;
    visit.current?.abort();
    const request = new AbortController();
    visit.current = request;
    setState((current) => ({
      ...(current.key === key ? current : empty(key)),
      loading: true,
      error: false,
    }));
    void (async () => {
      const pages: LedgerEvent[][] = [];
      let before = "";
      let truncated = false;
      for (let page = 0; ; page += 1) {
        if (page >= MAX_TASK_SEARCH_PAGES) {
          truncated = true;
          break;
        }
        const response = await searchChatMessages(
          groupId,
          "",
          { taskId, limit: PAGE_SIZE, before: before || undefined },
          { signal: request.signal },
        );
        if (request.signal.aborted) return;
        if (!response.ok) {
          setState((current) => ({ ...current, loading: false, error: true }));
          return;
        }
        const events = response.result.events ?? [];
        pages.unshift(events);
        const oldest = String(events[0]?.id || "");
        if (!response.result.has_more || !oldest) break;
        before = oldest;
      }
      setState({
        key,
        events: pages.flat().filter(isFormalChatMessageEvent).filter(shouldShowInConversation),
        loaded: true,
        loading: false,
        error: false,
        truncated,
      });
    })();
  }, [groupId, key, taskId]);

  useEffect(() => {
    if (!key) {
      setState(empty(""));
      return;
    }
    load();
    return () => visit.current?.abort();
  }, [key, load]);

  const current = state.key === key ? state : empty(key);
  return {
    active: !!key,
    events: current.events,
    loaded: current.loaded,
    loading: current.loading || (!!key && !current.loaded && !current.error),
    error: current.error,
    truncated: current.truncated,
    refresh: load,
  };
}
