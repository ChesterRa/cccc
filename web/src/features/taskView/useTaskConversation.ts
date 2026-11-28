import { useCallback, useEffect, useRef, useState } from "react";
import {
  shouldShowInConversation,
  toVisibleConversationEvent,
} from "../../hooks/chat/chatTabBasics";
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
 * Every message in the Group's history whose record contains the task's ID, read from the
 * server search, newest page first. The server matches the ID anywhere in a record, so the
 * caller still decides which of these belong to the task.
 */
export function useTaskConversation(groupId: string, taskId: string) {
  const key = groupId && taskId ? JSON.stringify([groupId, taskId]) : "";
  const [state, setState] = useState<State>(() => empty(key));
  const visit = useRef(0);

  const load = useCallback(() => {
    if (!key) return;
    const request = ++visit.current;
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
        const response = await searchChatMessages(groupId, taskId, {
          limit: PAGE_SIZE,
          before: before || undefined,
        });
        if (request !== visit.current) return;
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
        events: pages
          .flat()
          .filter(isFormalChatMessageEvent)
          .filter(shouldShowInConversation)
          .map(toVisibleConversationEvent),
        loaded: true,
        loading: false,
        error: false,
        truncated,
      });
    })();
  }, [groupId, key, taskId]);

  useEffect(() => {
    if (!key) {
      visit.current += 1;
      setState(empty(""));
      return;
    }
    load();
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
