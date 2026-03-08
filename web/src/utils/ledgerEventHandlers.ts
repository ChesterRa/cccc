// Ledger event handlers - pure functions for processing SSE events
// Extracted from useSSE.ts for better testability and separation of concerns

import type { LedgerEvent, Actor, ChatMessageData } from "../types";

// ============ Type Guards ============

interface BaseLedgerEvent {
  kind: string;
  data?: unknown;
  by?: string;
  id?: string;
}

export function isContextSyncEvent(ev: unknown): ev is BaseLedgerEvent & { kind: "context.sync" } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "context.sync";
}

export function isChatReadEvent(
  ev: unknown
): ev is BaseLedgerEvent & { kind: "chat.read"; data: { actor_id?: string; event_id?: string } } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "chat.read";
}

export function isChatAckEvent(
  ev: unknown
): ev is BaseLedgerEvent & { kind: "chat.ack"; data: { actor_id?: string; event_id?: string } } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "chat.ack";
}

export function isChatMessageEvent(ev: unknown): ev is LedgerEvent & { kind: "chat.message"; data: ChatMessageData } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "chat.message";
}

export function isActorActivityEvent(
  ev: unknown,
): ev is BaseLedgerEvent & { kind: "actor.activity"; data: { actors: Array<{ id: string; idle_seconds: number; running: boolean }> } } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "actor.activity";
}

// ============ Recipient Resolution ============

/**
 * Compute recipient actor IDs for a chat message (for read status tracking).
 */
export function getRecipientActorIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

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

/**
 * Compute recipient IDs for ACK tracking (attention messages only).
 * Includes "user" when explicitly targeted.
 */
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

// ============ Event Processors ============

export interface ChatReadData {
  actorId: string;
  eventId: string;
}

/**
 * Extract read event data. Returns null if data is invalid.
 */
export function extractChatReadData(ev: unknown): ChatReadData | null {
  if (!isChatReadEvent(ev)) return null;
  const actorId = String(ev.data?.actor_id || "");
  const eventId = String(ev.data?.event_id || "");
  if (!actorId || !eventId) return null;
  return { actorId, eventId };
}

/**
 * Extract ack event data. Returns null if data is invalid.
 */
export function extractChatAckData(ev: unknown): ChatReadData | null {
  if (!isChatAckEvent(ev)) return null;
  const actorId = String(ev.data?.actor_id || "");
  const eventId = String(ev.data?.event_id || "");
  if (!actorId || !eventId) return null;
  return { actorId, eventId };
}

/**
 * Initialize read status for a new chat message event.
 * Mutates the event object to add _read_status.
 */
export function initializeReadStatus(ev: LedgerEvent, actors: Actor[]): void {
  if (!isChatMessageEvent(ev)) return;
  if (ev._read_status) return; // Already initialized

  const recipients = getRecipientActorIdsForEvent(ev, actors);
  if (recipients.length > 0) {
    const rs: Record<string, boolean> = {};
    for (const id of recipients) rs[id] = false;
    ev._read_status = rs;
  }
}

/**
 * Initialize ack status for attention messages.
 * Mutates the event object to add _ack_status.
 */
export function initializeAckStatus(ev: LedgerEvent, actors: Actor[]): void {
  if (!isChatMessageEvent(ev)) return;
  if (ev._ack_status) return; // Already initialized

  const msgData = ev.data as ChatMessageData | undefined;
  if (String(msgData?.priority || "normal") !== "attention") return;

  const recipients = getAckRecipientIdsForEvent(ev, actors);
  if (recipients.length > 0) {
    const as: Record<string, boolean> = {};
    for (const id of recipients) as[id] = false;
    ev._ack_status = as;
  }
}

/**
 * Initialize obligation status for chat messages.
 * Mutates the event object to add _obligation_status.
 */
export function initializeObligationStatus(ev: LedgerEvent, actors: Actor[]): void {
  if (!isChatMessageEvent(ev)) return;
  if (ev._obligation_status) return;

  const msgData = ev.data as ChatMessageData | undefined;
  const recipients = getAckRecipientIdsForEvent(ev, actors);
  if (recipients.length <= 0) return;

  const status: Record<string, { read: boolean; acked: boolean; replied: boolean; reply_required: boolean }> = {};
  const isAttention = String(msgData?.priority || "normal") === "attention";
  const replyRequired = !!msgData?.reply_required;

  for (const rid of recipients) {
    status[rid] = {
      read: false,
      acked: !isAttention,
      replied: false,
      reply_required: replyRequired,
    };
  }

  ev._obligation_status = status;
}

/**
 * Check if a chat message should increment unread count.
 */
export function shouldIncrementUnread(
  ev: LedgerEvent,
  chatActive: boolean,
  atBottom: boolean
): boolean {
  if (!isChatMessageEvent(ev)) return false;
  const by = String(ev.by || "");
  if (!by || by === "user") return false;
  return !chatActive || !atBottom;
}

/**
 * Event kinds that should trigger actor refresh.
 */
const ACTOR_REFRESH_EVENTS = new Set([
  "chat.read",
  "system.notify",
  "system.notify_ack",
  "group.start",
  "group.stop",
  "group.set_state",
]);

/**
 * Check if an event should trigger actor refresh.
 */
export function shouldRefreshActors(ev: unknown): boolean {
  if (ev === null || typeof ev !== "object") return false;
  const kind = String((ev as BaseLedgerEvent).kind || "");
  if (ACTOR_REFRESH_EVENTS.has(kind)) return true;
  if (kind.startsWith("actor.")) return true;
  return false;
}
