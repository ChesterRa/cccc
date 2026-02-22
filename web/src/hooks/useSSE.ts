// SSE connection management for the ledger stream.
import { useEffect, useRef } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { Actor, GroupContext } from "../types";
import {
  isContextSyncEvent,
  isChatReadEvent,
  isChatAckEvent,
  isChatMessageEvent,
  extractChatReadData,
  extractChatAckData,
  initializeReadStatus,
  initializeAckStatus,
  initializeObligationStatus,
  shouldIncrementUnread,
  shouldRefreshActors,
  // Re-export for consumers
  getRecipientActorIdsForEvent,
  getAckRecipientIdsForEvent,
} from "../utils/ledgerEventHandlers";

// Re-export for backward compatibility
export { getRecipientActorIdsForEvent, getAckRecipientIdsForEvent };

interface UseSSEOptions {
  activeTabRef: React.MutableRefObject<string>;
  chatAtBottomRef: React.MutableRefObject<boolean>;
  actorsRef: React.MutableRefObject<Actor[]>;
}

export function useSSE({ activeTabRef, chatAtBottomRef, actorsRef }: UseSSEOptions) {
  const {
    selectedGroupId,
    appendEvent,
    updateReadStatus,
    updateAckStatus,
    updateReplyStatus,
    setGroupContext,
    refreshActors,
  } = useGroupStore();

  const { incrementChatUnread, setSSEStatus } = useUIStore();

  const eventSourceRef = useRef<EventSource | null>(null);
  const contextRefreshTimerRef = useRef<number | null>(null);
  const actorWarmupTimersRef = useRef<number[]>([]);
  const selectedGroupIdRef = useRef<string>("");
  const reconnectDelayRef = useRef<number>(1000);
  const reconnectTimerRef = useRef<number | null>(null);
  const hasConnectedOnceRef = useRef<boolean>(false);

  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  async function fetchContext(groupId: string) {
    const resp = await api.fetchContext(groupId);
    if (resp.ok && resp.result && typeof resp.result === "object") {
      setGroupContext(resp.result as GroupContext);
    }
  }

  function connectStream(groupId: string) {
    if (reconnectTimerRef.current) {
      window.clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }

    setSSEStatus("connecting");
    const es = new EventSource(api.withAuthToken(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/stream`));

    const isReconnect = hasConnectedOnceRef.current;

    es.onopen = () => {
      reconnectDelayRef.current = 1000;
      setSSEStatus("connected");
      hasConnectedOnceRef.current = true;

      // On reconnect, reload events to fill the gap from the disconnect window.
      // The backend SSE stream seeks to EOF on new connections, so any events
      // written during the disconnect period are missed. loadGroup re-fetches
      // the latest events via HTTP to compensate.
      if (isReconnect) {
        void useGroupStore.getState().loadGroup(groupId);
      }
    };

    es.onerror = () => {
      es.close();
      eventSourceRef.current = null;
      setSSEStatus("disconnected");

      const delay = reconnectDelayRef.current;
      reconnectTimerRef.current = window.setTimeout(() => {
        reconnectTimerRef.current = null;
        if (selectedGroupIdRef.current === groupId) {
          connectStream(groupId);
        }
      }, delay);

      reconnectDelayRef.current = Math.min(delay * 2, 30000);
    };

    es.addEventListener("ledger", (e) => {
      const msg = e as MessageEvent;
      try {
        const ev = JSON.parse(String(msg.data || "{}"));

        // Context sync - debounced refresh
        if (isContextSyncEvent(ev)) {
          if (contextRefreshTimerRef.current) window.clearTimeout(contextRefreshTimerRef.current);
          contextRefreshTimerRef.current = window.setTimeout(() => {
            contextRefreshTimerRef.current = null;
            void fetchContext(groupId);
          }, 150);
          return;
        }

        // Chat read status update
        if (isChatReadEvent(ev)) {
          const data = extractChatReadData(ev);
          if (data) {
            updateReadStatus(data.eventId, data.actorId);
          }
          void refreshActors(groupId);
          return;
        }

        // Chat ack status update
        if (isChatAckEvent(ev)) {
          const data = extractChatAckData(ev);
          if (data) {
            updateAckStatus(data.eventId, data.actorId);
          }
          return;
        }

        // Initialize read/ack status for new messages
        initializeReadStatus(ev, actorsRef.current);
        initializeAckStatus(ev, actorsRef.current);
        initializeObligationStatus(ev, actorsRef.current);

        appendEvent(ev);

        // Reply to an earlier message updates its obligation status in-place.
        if (isChatMessageEvent(ev)) {
          const msgData = ev.data && typeof ev.data === "object" ? (ev.data as { reply_to?: unknown }) : null;
          const replyTo = msgData && typeof msgData.reply_to === "string" ? String(msgData.reply_to || "").trim() : "";
          const replyBy = String(ev.by || "").trim();
          if (replyTo && replyBy) {
            updateReplyStatus(replyTo, replyBy);
          }
        }

        // Update unread count
        if (shouldIncrementUnread(ev, activeTabRef.current === "chat", chatAtBottomRef.current)) {
          incrementChatUnread();
        }

        // Refresh actors when relevant events arrive
        if (shouldRefreshActors(ev)) {
          void refreshActors(groupId);
        }
      } catch {
        /* ignore parse errors */
      }
    });
    eventSourceRef.current = es;
  }

  function clearActorWarmupTimers() {
    for (const t of actorWarmupTimersRef.current) window.clearTimeout(t);
    actorWarmupTimersRef.current = [];
  }

  function scheduleActorWarmupRefresh(groupId: string) {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    clearActorWarmupTimers();
    const delaysMs = [3000, 8000, 15000];
    for (const ms of delaysMs) {
      const t = window.setTimeout(() => {
        if (selectedGroupIdRef.current !== gid) return;
        void refreshActors(gid);
      }, ms);
      actorWarmupTimersRef.current.push(t);
    }
  }

  function cleanup() {
    if (reconnectTimerRef.current) {
      window.clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    if (contextRefreshTimerRef.current) {
      window.clearTimeout(contextRefreshTimerRef.current);
      contextRefreshTimerRef.current = null;
    }
    clearActorWarmupTimers();
    reconnectDelayRef.current = 1000;
    hasConnectedOnceRef.current = false;
    setSSEStatus("disconnected");
  }

  return {
    connectStream,
    fetchContext,
    scheduleActorWarmupRefresh,
    clearActorWarmupTimers,
    cleanup,
    contextRefreshTimerRef,
  };
}
