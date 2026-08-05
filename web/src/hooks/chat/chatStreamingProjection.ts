import type { ChatMessageData, LedgerEvent } from "../../types";
import { hasRenderableChatMessageContent } from "../../utils/ledgerEventHandlers";
import { mergeStreamingCandidates } from "./chatTabBasics";
import { getReplySlotKey } from "./chatReplySlots";

function getNormalizedStreamPhase(data: { stream_phase?: unknown } | null | undefined): string {
  return String(data?.stream_phase || "")
    .trim()
    .toLowerCase();
}

function hasExplicitStreamingPhase(data: { stream_phase?: unknown } | null | undefined): boolean {
  const streamPhase = getNormalizedStreamPhase(data);
  return streamPhase === "commentary" || streamPhase === "final_answer";
}

export function isPlaceholderLikeStreamingEvent(
  data: ChatMessageData & {
    pending_placeholder?: unknown;
    stream_id?: unknown;
    stream_phase?: unknown;
    text?: unknown;
    activities?: unknown;
  },
): boolean {
  const streamId = String(data.stream_id || "").trim();
  if (data.pending_placeholder) return true;

  if (hasExplicitStreamingPhase(data)) return false;

  const text = typeof data.text === "string" ? data.text.trim() : "";
  if (text) return false;
  if (!hasOnlyQueuedActivities(data.activities)) return false;

  return streamId.startsWith("local:") || streamId.startsWith("pending:");
}

export function hasOnlyQueuedActivities(value: unknown): boolean {
  const activities = Array.isArray(value) ? value : [];
  return (
    activities.length === 0 ||
    activities.every((item) => {
      if (!item || typeof item !== "object") return true;
      const kind = String((item as { kind?: unknown }).kind || "").trim();
      const summary = String((item as { summary?: unknown }).summary || "").trim();
      return kind === "queued" && summary === "queued";
    })
  );
}

export function hasRichActivities(value: unknown): boolean {
  const activities = Array.isArray(value) ? value : [];
  return activities.some((item) => {
    if (!item || typeof item !== "object") return false;
    const kind = String((item as { kind?: unknown }).kind || "").trim();
    const summary = String((item as { summary?: unknown }).summary || "").trim();
    return kind !== "queued" || summary !== "queued";
  });
}

function getStreamingEventDedupeKey(event: LedgerEvent): string {
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & {
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
          stream_id?: unknown;
        })
      : {};
  const actorId = String(event.by || "").trim();
  const pendingEventId = String(data.pending_event_id || "").trim();
  const streamId = String(data.stream_id || "").trim();
  if (!actorId) return "";
  // Placeholder lifecycle events still collapse by pending reply slot, but a
  // real text-bearing stream must keep stream_id identity or short streaming
  // messages will overwrite each other before they ever reach the list.
  if (
    pendingEventId &&
    (!hasRenderableChatMessageContent(event) || isPlaceholderLikeStreamingEvent(data))
  ) {
    return `pending:${actorId}:${pendingEventId}`;
  }
  if (streamId) {
    return `stream:${actorId}:${streamId}`;
  }
  if (pendingEventId) {
    return `pending:${actorId}:${pendingEventId}`;
  }
  return "";
}

export function dedupeStreamingEvents(streamingEvents: LedgerEvent[]): LedgerEvent[] {
  const byKey = new Map<string, LedgerEvent>();
  const passthrough: LedgerEvent[] = [];

  for (const event of streamingEvents) {
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as ChatMessageData & {
            pending_placeholder?: unknown;
            pending_event_id?: unknown;
            stream_id?: unknown;
          })
        : {};
    const streamId = String(data.stream_id || "").trim();
    const isPendingPlaceholder = Boolean(data.pending_placeholder);
    const dedupeKey = getStreamingEventDedupeKey(event);

    if (!dedupeKey) {
      passthrough.push(event);
      continue;
    }

    const existing = byKey.get(dedupeKey);
    if (!existing) {
      byKey.set(dedupeKey, event);
      continue;
    }

    const existingData =
      existing.data && typeof existing.data === "object"
        ? (existing.data as ChatMessageData & {
            pending_placeholder?: unknown;
            pending_event_id?: unknown;
            stream_id?: unknown;
          })
        : {};
    const existingIsPendingPlaceholder = Boolean(existingData.pending_placeholder);
    const preferCurrent =
      existingIsPendingPlaceholder && !isPendingPlaceholder
        ? true
        : existingIsPendingPlaceholder === isPendingPlaceholder &&
          !!streamId &&
          !String(existingData.stream_id || "").trim();

    byKey.set(
      dedupeKey,
      preferCurrent
        ? mergeStreamingCandidates(event, existing)
        : mergeStreamingCandidates(existing, event),
    );
  }

  return [...passthrough, ...byKey.values()];
}

export function collapseActorStreamingPlaceholders(streamingEvents: LedgerEvent[]): LedgerEvent[] {
  const eventsByActor = new Map<string, LedgerEvent[]>();
  for (const event of streamingEvents) {
    const actorId = String(event.by || "").trim();
    if (!actorId) continue;
    const bucket = eventsByActor.get(actorId);
    if (bucket) {
      bucket.push(event);
    } else {
      eventsByActor.set(actorId, [event]);
    }
  }

  const shouldDrop = new Set<LedgerEvent>();
  for (const actorEvents of eventsByActor.values()) {
    if (actorEvents.length <= 1) continue;

    const richReplySlots = new Set<string>();
    actorEvents.forEach((event) => {
      const data =
        event.data && typeof event.data === "object"
          ? (event.data as ChatMessageData & { activities?: unknown[] })
          : {};
      const text = typeof data.text === "string" ? data.text.trim() : "";
      const activities = Array.isArray(data.activities) ? data.activities : [];
      const hasRichStreaming =
        text.length > 0 ||
        activities.some((item) => {
          if (!item || typeof item !== "object") return false;
          const kind = String((item as { kind?: unknown }).kind || "").trim();
          const summary = String((item as { summary?: unknown }).summary || "").trim();
          return kind !== "queued" || summary !== "queued";
        });
      if (!hasRichStreaming) return;
      const slotKey = getReplySlotKey(event);
      if (slotKey) {
        richReplySlots.add(slotKey);
      }
    });

    if (richReplySlots.size > 0) {
      for (const event of actorEvents) {
        const slotKey = getReplySlotKey(event);
        if (!slotKey || !richReplySlots.has(slotKey)) continue;
        const data =
          event.data && typeof event.data === "object"
            ? (event.data as ChatMessageData & {
                pending_placeholder?: unknown;
                activities?: unknown[];
                stream_id?: unknown;
                stream_phase?: unknown;
              })
            : {};
        const text = typeof data.text === "string" ? data.text.trim() : "";
        const onlyQueuedActivities = hasOnlyQueuedActivities(data.activities);
        const isPlaceholderLike = isPlaceholderLikeStreamingEvent(data);
        if (
          !text &&
          !hasRichActivities(data.activities) &&
          (isPlaceholderLike || (onlyQueuedActivities && !hasExplicitStreamingPhase(data)))
        ) {
          shouldDrop.add(event);
        }
      }
      continue;
    }

    const placeholderOnlyEvents = actorEvents.filter((event) => {
      const data =
        event.data && typeof event.data === "object"
          ? (event.data as ChatMessageData & {
              pending_placeholder?: unknown;
              stream_id?: unknown;
              stream_phase?: unknown;
            })
          : {};
      const text = typeof data.text === "string" ? data.text.trim() : "";
      if (text) return false;
      const onlyQueuedActivities = hasOnlyQueuedActivities(data.activities);
      return onlyQueuedActivities && isPlaceholderLikeStreamingEvent(data);
    });
    if (placeholderOnlyEvents.length <= 1) continue;
    const latestPlaceholder = placeholderOnlyEvents.reduce((latest, current) => {
      const latestTs = String(latest.ts || "");
      const currentTs = String(current.ts || "");
      return currentTs >= latestTs ? current : latest;
    });
    for (const event of placeholderOnlyEvents) {
      if (event !== latestPlaceholder) {
        shouldDrop.add(event);
      }
    }
  }

  return streamingEvents.filter((event) => !shouldDrop.has(event));
}

export function dropOrphanQueuedPlaceholders(
  canonicalEvents: LedgerEvent[],
  streamingEvents: LedgerEvent[],
): LedgerEvent[] {
  const renderableCanonicalReplySlots = new Set(
    canonicalEvents
      .filter((event) => hasRenderableChatMessageContent(event))
      .map((event) => getReplySlotKey(event))
      .filter((slotKey) => slotKey.length > 0),
  );

  return streamingEvents.filter((event) => {
    const slotKey = getReplySlotKey(event);
    if (!slotKey || !renderableCanonicalReplySlots.has(slotKey)) return true;
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as ChatMessageData & {
            pending_placeholder?: unknown;
            stream_id?: unknown;
            stream_phase?: unknown;
          })
        : {};
    const text = typeof data.text === "string" ? data.text.trim() : "";
    if (text) return true;
    const isPlaceholderLike = isPlaceholderLikeStreamingEvent(data);
    return !(isPlaceholderLike && !hasRichActivities(data.activities));
  });
}

export function getCanonicalStreamingSupersededStreamIds(
  canonicalEvents: LedgerEvent[],
): Set<string> {
  return new Set(
    canonicalEvents
      .filter((event) => hasRenderableChatMessageContent(event))
      .map((event) => {
        const data =
          event.data && typeof event.data === "object"
            ? (event.data as { stream_id?: unknown })
            : null;
        return data && typeof data.stream_id === "string" ? data.stream_id.trim() : "";
      })
      .filter((streamId) => streamId.length > 0),
  );
}
