// SSE connection management for the ledger stream.
import { useEffect, useRef } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { LedgerEvent, Actor, ChatMessageData, GroupContext } from "../types";

// Compute recipient actor IDs for a chat message (exported for reuse).
export function getRecipientActorIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

  // Treat data as ChatMessageData.
  const msgData = ev.data as ChatMessageData | undefined;
  const toRaw = msgData && Array.isArray(msgData.to) ? msgData.to : [];
  const tokens = (toRaw as unknown[])
    .map((x) => String(x || "").trim())
    .filter((s) => s.length > 0);
  const tokenSet = new Set(tokens);

  const by = String(ev.by || "").trim();

  if (tokenSet.size === 0 || tokenSet.has("@all")) {
    return actorIds.filter((id) => id !== by);
  }

  const out = new Set<string>();
  for (const t of tokenSet) {
    if (t === "user" || t === "@user") continue;
    if (t === "@peers") {
      for (const a of actors) {
        if (a.role === "peer") out.add(String(a.id));
      }
      continue;
    }
    if (t === "@foreman") {
      for (const a of actors) {
        if (a.role === "foreman") out.add(String(a.id));
      }
      continue;
    }
    if (actorIdSet.has(t)) out.add(t);
  }

  out.delete(by);
  return Array.from(out);
}

export function getAckRecipientIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

  const msgData = ev.data as ChatMessageData | undefined;
  const dst = typeof msgData?.dst_group_id === "string" ? String(msgData.dst_group_id || "").trim() : "";
  if (dst) return [];
  const toRaw = msgData && Array.isArray(msgData.to) ? msgData.to : [];
  const tokens = (toRaw as unknown[])
    .map((x) => String(x || "").trim())
    .filter((s) => s.length > 0);
  const tokenSet = new Set(tokens);

  const by = String(ev.by || "").trim();

  const out = new Set<string>();

  if (tokenSet.size === 0 || tokenSet.has("@all")) {
    for (const id of actorIds) {
      if (id && id !== by) out.add(id);
    }
  } else {
    for (const t of tokenSet) {
      if (t === "@peers") {
        for (const a of actors) {
          if (a.role === "peer") out.add(String(a.id));
        }
        continue;
      }
      if (t === "@foreman") {
        for (const a of actors) {
          if (a.role === "foreman") out.add(String(a.id));
        }
        continue;
      }
      if (t === "user" || t === "@user") continue;
      if (actorIdSet.has(t)) out.add(t);
    }
  }

  // "user" ACK is only required when explicitly targeted.
  if (by !== "user" && (tokenSet.has("user") || tokenSet.has("@user"))) {
    out.add("user");
  }

  out.delete(by);
  return Array.from(out);
}

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
    setGroupContext,
    refreshActors,
  } = useGroupStore();

  const { incrementChatUnread } = useUIStore();

  const eventSourceRef = useRef<EventSource | null>(null);
  const contextRefreshTimerRef = useRef<number | null>(null);
  const actorWarmupTimersRef = useRef<number[]>([]);
  const selectedGroupIdRef = useRef<string>("");

  // Keep a ref in sync to avoid stale closures.
  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  // Fetch Context
  async function fetchContext(groupId: string) {
    const resp = await api.fetchContext(groupId);
    if (resp.ok && resp.result && typeof resp.result === "object") {
      setGroupContext(resp.result as GroupContext);
    }
  }

  // Connect SSE stream for a group.
  function connectStream(groupId: string) {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }

    const es = new EventSource(api.withAuthToken(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/stream`));
    es.addEventListener("ledger", (e) => {
      const msg = e as MessageEvent;
      try {
        const ev = JSON.parse(String(msg.data || "{}"));

        // Context sync event
        if (ev && typeof ev === "object" && ev.kind === "context.sync") {
          if (contextRefreshTimerRef.current) window.clearTimeout(contextRefreshTimerRef.current);
          contextRefreshTimerRef.current = window.setTimeout(() => {
            contextRefreshTimerRef.current = null;
            void fetchContext(groupId);
          }, 150);
          return;
        }

        // Chat read event
        if (ev && typeof ev === "object" && ev.kind === "chat.read") {
          const actorId = String(ev.data?.actor_id || "");
          const eventId = String(ev.data?.event_id || "");
          if (actorId && eventId) {
            updateReadStatus(eventId, actorId);
          }
          void refreshActors(groupId);
          return;
        }

        // Chat ack event (for attention messages)
        if (ev && typeof ev === "object" && ev.kind === "chat.ack") {
          const actorId = String(ev.data?.actor_id || "");
          const eventId = String(ev.data?.event_id || "");
          if (actorId && eventId) {
            updateAckStatus(eventId, actorId);
          }
          return;
        }

        // Chat message: initialize read-status keys (for live ✓ updates).
        if (ev && typeof ev === "object" && ev.kind === "chat.message" && !ev._read_status) {
          const recipients = getRecipientActorIdsForEvent(ev, actorsRef.current);
          if (recipients.length > 0) {
            const rs: Record<string, boolean> = {};
            for (const id of recipients) rs[id] = false;
            ev._read_status = rs;
          }
        }

        // Chat message: initialize ack-status keys for attention messages (for live ACK updates).
        if (
          ev &&
          typeof ev === "object" &&
          ev.kind === "chat.message" &&
          String(ev.data?.priority || "normal") === "attention" &&
          !ev._ack_status
        ) {
          const recipients = getAckRecipientIdsForEvent(ev, actorsRef.current);
          if (recipients.length > 0) {
            const as: Record<string, boolean> = {};
            for (const id of recipients) as[id] = false;
            ev._ack_status = as;
          }
        }

        appendEvent(ev);

        // Update unread count
        if (ev && typeof ev === "object" && ev.kind === "chat.message") {
          const by = String(ev.by || "");
          if (by && by !== "user") {
            const chatActive = activeTabRef.current === "chat";
            const atBottom = chatAtBottomRef.current;
            if (!chatActive || !atBottom) {
              incrementChatUnread();
            }
          }
        }

        // Refresh actors when relevant events arrive
        const kind = String(ev.kind || "");
        if (
          kind === "chat.message" ||
          kind === "chat.read" ||
          kind.startsWith("actor.") ||
          kind === "group.start" ||
          kind === "group.stop" ||
          kind === "group.set_state"
        ) {
          void refreshActors(groupId);
        }
      } catch {
        /* ignore */
      }
    });
    eventSourceRef.current = es;
  }

  // Actor warmup refresh (helps smooth startup state transitions).
  function clearActorWarmupTimers() {
    for (const t of actorWarmupTimersRef.current) window.clearTimeout(t);
    actorWarmupTimersRef.current = [];
  }

  function scheduleActorWarmupRefresh(groupId: string) {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    clearActorWarmupTimers();
    const delaysMs = [1000, 2500, 5000, 10000, 15000];
    for (const ms of delaysMs) {
      const t = window.setTimeout(() => {
        if (selectedGroupIdRef.current !== gid) return;
        void refreshActors(gid);
      }, ms);
      actorWarmupTimersRef.current.push(t);
    }
  }

  // Cleanup
  function cleanup() {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    if (contextRefreshTimerRef.current) {
      window.clearTimeout(contextRefreshTimerRef.current);
      contextRefreshTimerRef.current = null;
    }
    clearActorWarmupTimers();
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
