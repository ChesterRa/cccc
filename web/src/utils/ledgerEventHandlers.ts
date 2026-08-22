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

export function isMailReadEvent(
  ev: unknown,
): ev is BaseLedgerEvent & { kind: "mail.read"; data: { actor_id?: string; event_id?: string } } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "mail.read";
}

export function isRuntimeDeliveryEvent(
  ev: unknown,
): ev is BaseLedgerEvent & {
  kind: "runtime.delivery";
  data: { actor_id?: string; source_event_id?: string; state?: string };
} {
  return (
    ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "runtime.delivery"
  );
}

export function isReplyRequestCancelledEvent(
  ev: unknown,
): ev is BaseLedgerEvent & {
  kind: "chat.reply_request.cancelled";
  data: { source_event_id?: string };
} {
  return (
    ev !== null &&
    typeof ev === "object" &&
    (ev as BaseLedgerEvent).kind === "chat.reply_request.cancelled"
  );
}

export function isChatMessageEvent(
  ev: unknown,
): ev is LedgerEvent & { kind: "chat.message"; data: ChatMessageData } {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "chat.message";
}

export function hasRenderableChatMessageContent(event: {
  kind?: unknown;
  data?: unknown;
}): boolean {
  if (String(event.kind || "").trim() !== "chat.message") return false;
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as { text?: unknown; attachments?: unknown; refs?: unknown })
      : null;
  const text = typeof data?.text === "string" ? data.text.trim() : "";
  if (text) return true;
  const attachments = Array.isArray(data?.attachments) ? data.attachments : [];
  if (attachments.length > 0) return true;
  const refs = Array.isArray(data?.refs) ? data.refs : [];
  return refs.length > 0;
}

export function isActorActivityEvent(
  ev: unknown,
): ev is BaseLedgerEvent & {
  kind: "actor.activity";
  data: {
    actors: Array<{
      id: string;
      idle_seconds?: number | null;
      running: boolean;
      effective_working_state?: string;
      effective_working_reason?: string;
      effective_working_updated_at?: string | null;
      effective_active_task_id?: string | null;
    }>;
  };
} {
  return ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "actor.activity";
}

export function isPresentationPublishEvent(
  ev: unknown,
): ev is BaseLedgerEvent & {
  kind: "presentation.publish";
  data: { slot_id?: string; title?: string; card_type?: string };
} {
  return (
    ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "presentation.publish"
  );
}

export function isPresentationClearEvent(
  ev: unknown,
): ev is BaseLedgerEvent & {
  kind: "presentation.clear";
  data: { slot_id?: string; cleared_slots?: string[] };
} {
  return (
    ev !== null && typeof ev === "object" && (ev as BaseLedgerEvent).kind === "presentation.clear"
  );
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
 * Compute recipient IDs for message status tracking.
 * Includes "user" when explicitly targeted.
 */
export function getStatusRecipientIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

  const msgData = ev.data as ChatMessageData | undefined;
  const dst =
    typeof msgData?.dst_group_id === "string" ? String(msgData.dst_group_id || "").trim() : "";
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

  // User status exists only when the user is explicitly targeted.
  if (by !== "user" && (tokenSet.has("user") || tokenSet.has("@user"))) {
    out.add("user");
  }

  out.delete(by);
  return Array.from(out);
}

// ============ Event Processors ============

export interface MailReadData {
  actorId: string;
  eventId: string;
}

export interface RuntimeDeliveryData {
  actorId: string;
  eventId: string;
  state: string;
}

/**
 * Extract read event data. Returns null if data is invalid.
 */
export function extractMailReadData(ev: unknown): MailReadData | null {
  if (!isMailReadEvent(ev)) return null;
  const actorId = String(ev.data?.actor_id || "");
  const eventId = String(ev.data?.event_id || "");
  if (!actorId || !eventId) return null;
  return { actorId, eventId };
}

export function extractRuntimeDeliveryData(ev: unknown): RuntimeDeliveryData | null {
  if (!isRuntimeDeliveryEvent(ev)) return null;
  const actorId = String(ev.data?.actor_id || "").trim();
  const eventId = String(ev.data?.source_event_id || "").trim();
  const state = String(ev.data?.state || "").trim();
  if (!actorId || !eventId || !state) return null;
  return { actorId, eventId, state };
}

export function extractCancelledSourceEventId(ev: unknown): string {
  if (!isReplyRequestCancelledEvent(ev)) return "";
  return String(ev.data?.source_event_id || "").trim();
}

/**
 * Initialize read status for a new chat message event.
 * Mutates the event object to add _read_status.
 */
export function initializeReadStatus(ev: LedgerEvent, actors: Actor[]): void {
  if (!isChatMessageEvent(ev)) return;
  if ((ev.data as ChatMessageData | undefined)?.message_mode !== "mail") return;
  if (ev._read_status) return; // Already initialized

  const recipients = getRecipientActorIdsForEvent(ev, actors);
  if (recipients.length > 0) {
    const rs: Record<string, boolean> = {};
    for (const id of recipients) rs[id] = false;
    ev._read_status = rs;
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
  const recipients = getStatusRecipientIdsForEvent(ev, actors);
  if (recipients.length <= 0) return;

  const status: NonNullable<LedgerEvent["_obligation_status"]> = {};
  const replyRequested = msgData?.message_mode === "request_reply";

  for (const rid of recipients) {
    status[rid] = {
      replied: false,
      reply_requested: replyRequested,
      cancelled: false,
      delivery_state: "",
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
  atBottom: boolean,
): boolean {
  if (!isChatMessageEvent(ev)) return false;
  const by = String(ev.by || "");
  if (!by || by === "user") return false;
  return !chatActive || !atBottom;
}

/**
 * Event kinds that should trigger actor refresh.
 */
const ACTOR_READONLY_REFRESH_EVENTS = new Set(["group.start", "group.stop", "group.set_state"]);

const ACTOR_UNREAD_REFRESH_EVENTS = new Set(["mail.read"]);

export type ActorRefreshMode = "none" | "readonly" | "unread";

export function getActorRefreshMode(ev: unknown): ActorRefreshMode {
  if (ev === null || typeof ev !== "object") return "none";
  const kind = String((ev as BaseLedgerEvent).kind || "");
  if (ACTOR_UNREAD_REFRESH_EVENTS.has(kind)) return "unread";
  if (ACTOR_READONLY_REFRESH_EVENTS.has(kind)) return "readonly";
  if (kind.startsWith("actor.")) return "readonly";
  return "none";
}
