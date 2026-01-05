// SSE 连接管理 hook
import { useEffect, useRef } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { LedgerEvent, Actor, ChatMessageData, GroupContext } from "../types";

// 获取消息的接收者 Actor IDs (导出供其他组件使用)
export function getRecipientActorIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

  // 将 data 作为 ChatMessageData 处理
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
    setGroupContext,
    refreshActors,
  } = useGroupStore();

  const { incrementChatUnread } = useUIStore();

  const eventSourceRef = useRef<EventSource | null>(null);
  const contextRefreshTimerRef = useRef<number | null>(null);
  const actorWarmupTimersRef = useRef<number[]>([]);
  const selectedGroupIdRef = useRef<string>("");

  // 同步 ref
  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  // 获取 Context
  async function fetchContext(groupId: string) {
    const resp = await api.fetchContext(groupId);
    if (resp.ok && resp.result && typeof resp.result === "object") {
      setGroupContext(resp.result as GroupContext);
    }
  }

  // SSE 连接
  function connectStream(groupId: string) {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }

    const es = new EventSource(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/stream`);
    es.addEventListener("ledger", (e) => {
      const msg = e as MessageEvent;
      try {
        const ev = JSON.parse(String(msg.data || "{}"));

        // Context sync 事件
        if (ev && typeof ev === "object" && ev.kind === "context.sync") {
          if (contextRefreshTimerRef.current) window.clearTimeout(contextRefreshTimerRef.current);
          contextRefreshTimerRef.current = window.setTimeout(() => {
            contextRefreshTimerRef.current = null;
            void fetchContext(groupId);
          }, 150);
          return;
        }

        // Chat read 事件
        if (ev && typeof ev === "object" && ev.kind === "chat.read") {
          const actorId = String(ev.data?.actor_id || "");
          const eventId = String(ev.data?.event_id || "");
          if (actorId && eventId) {
            updateReadStatus(eventId, actorId);
          }
          void refreshActors(groupId);
          return;
        }

        // Chat message 事件 - 添加 read status
        if (ev && typeof ev === "object" && ev.kind === "chat.message" && !ev._read_status) {
          const recipients = getRecipientActorIdsForEvent(ev, actorsRef.current);
          if (recipients.length > 0) {
            const rs: Record<string, boolean> = {};
            for (const id of recipients) rs[id] = false;
            ev._read_status = rs;
          }
        }

        appendEvent(ev);

        // 更新未读计数
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

        // 刷新 actors
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

  // Actor warmup 定时刷新
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

  // 清理函数
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
