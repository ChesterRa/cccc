import type { LedgerEvent, StreamingActivity } from "../types";
import {
  type StreamingReplySession,
  dedupeStreamingActivities,
  normalizeStreamingActivityLog,
  normalizeReplySessionTimestamp,
  upsertReplySession,
  migrateReplySession,
} from "./chatStreamingSessions";

export type StreamingChatBucket = {
  events: LedgerEvent[];
  streamingEvents: LedgerEvent[];
  streamingTextByStreamId: Record<string, string>;
  streamingActivitiesByStreamId: Record<string, StreamingActivity[]>;
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
};

export type StreamingChatBucketPatch = Partial<StreamingChatBucket>;

function hasCanonicalReplyForPendingEvent(
  bucket: StreamingChatBucket,
  actorId: string,
  pendingEventId: string,
): boolean {
  const targetActorId = String(actorId || "").trim();
  const targetPendingEventId = String(pendingEventId || "").trim();
  if (!targetActorId || !targetPendingEventId) return false;
  return bucket.events.some((event) => {
    if (String(event.kind || "").trim() !== "chat.message") return false;
    if (String(event.by || "").trim() !== targetActorId) return false;
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as {
            pending_event_id?: unknown;
            reply_to?: unknown;
            text?: unknown;
            attachments?: unknown;
            refs?: unknown;
          })
        : {};
    const eventPendingEventId = String(data.pending_event_id || "").trim();
    const replyTo = String(data.reply_to || "").trim();
    if (eventPendingEventId !== targetPendingEventId && replyTo !== targetPendingEventId)
      return false;
    const text = String(data.text || "").trim();
    if (text) return true;
    const attachments = Array.isArray(data.attachments) ? data.attachments : [];
    if (attachments.length > 0) return true;
    const refs = Array.isArray(data.refs) ? data.refs : [];
    return refs.length > 0;
  });
}

function isQueuedOnlyActivityList(value: unknown): boolean {
  const activities = Array.isArray(value) ? value : [];
  return (
    activities.length === 0 ||
    activities.every((activity) => {
      if (!activity || typeof activity !== "object") return true;
      const kind = String((activity as { kind?: unknown }).kind || "")
        .trim()
        .toLowerCase();
      const summary = String((activity as { summary?: unknown }).summary || "")
        .trim()
        .toLowerCase();
      const status = String((activity as { status?: unknown }).status || "")
        .trim()
        .toLowerCase();
      return (
        kind === "queued" &&
        summary === "queued" &&
        (!status || status === "started" || status === "completed")
      );
    })
  );
}

function isBindablePendingStream(
  data: { pending_placeholder?: unknown; stream_id?: unknown; text?: unknown } | undefined,
): boolean {
  const streamId = String(data?.stream_id || "").trim();
  const text = String(data?.text || "").trim();
  return Boolean(data?.pending_placeholder) || (streamId.startsWith("pending:") && !text);
}

function findLatestBindableLocalPlaceholderIndex(
  streamingEvents: LedgerEvent[],
  actorId: string,
): number {
  const targetActorId = String(actorId || "").trim();
  if (!targetActorId) return -1;

  let bestIndex = -1;
  let bestTs = "";
  for (let index = 0; index < streamingEvents.length; index += 1) {
    const item = streamingEvents[index];
    if (String(item?.by || "").trim() !== targetActorId) continue;
    const data =
      item?.data && typeof item.data === "object"
        ? (item.data as {
            stream_id?: unknown;
            text?: unknown;
            pending_placeholder?: unknown;
            activities?: unknown;
          })
        : {};
    const streamId = String(data.stream_id || "").trim();
    const text = String(data.text || "").trim();
    const isPendingPlaceholder = Boolean(data.pending_placeholder);
    if (!streamId.startsWith("local:") || !isPendingPlaceholder || text) continue;
    if (!isQueuedOnlyActivityList(data.activities)) continue;
    const ts = String(item.ts || "").trim();
    if (bestIndex < 0 || ts >= bestTs) {
      bestIndex = index;
      bestTs = ts;
    }
  }
  return bestIndex;
}

function removeStreamBindings(
  bucket: StreamingChatBucket,
  removedStreamIds: string[],
): {
  streamingTextByStreamId: Record<string, string>;
  streamingActivitiesByStreamId: Record<string, StreamingActivity[]>;
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
} {
  const removedSet = new Set(
    removedStreamIds.map((value) => String(value || "").trim()).filter(Boolean),
  );
  const nextStreamingTextByStreamId = { ...bucket.streamingTextByStreamId };
  const nextStreamingActivitiesByStreamId = { ...bucket.streamingActivitiesByStreamId };
  const nextReplySessionsByPendingEventId = { ...bucket.replySessionsByPendingEventId };
  const nextPendingEventIdByStreamId = { ...bucket.pendingEventIdByStreamId };

  for (const streamId of removedSet) {
    delete nextStreamingTextByStreamId[streamId];
    delete nextStreamingActivitiesByStreamId[streamId];

    const pendingEventId = String(nextPendingEventIdByStreamId[streamId] || "").trim();
    delete nextPendingEventIdByStreamId[streamId];
    if (!pendingEventId) continue;

    const session = nextReplySessionsByPendingEventId[pendingEventId];
    if (!session) continue;

    const remainingStreamIds = Object.keys(nextPendingEventIdByStreamId).filter(
      (candidateStreamId) => nextPendingEventIdByStreamId[candidateStreamId] === pendingEventId,
    );
    if (remainingStreamIds.length <= 0) {
      delete nextReplySessionsByPendingEventId[pendingEventId];
      continue;
    }

    nextReplySessionsByPendingEventId[pendingEventId] = {
      ...session,
      currentStreamId: remainingStreamIds.includes(String(session.currentStreamId || "").trim())
        ? session.currentStreamId
        : remainingStreamIds[remainingStreamIds.length - 1],
    };
  }

  return {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function upsertStreamingEventPatch(
  bucket: StreamingChatBucket,
  groupId: string,
  event: LedgerEvent,
): StreamingChatBucketPatch | null {
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as {
          stream_id?: unknown;
          pending_event_id?: unknown;
          text?: unknown;
          activities?: unknown;
        })
      : {};
  const streamId = String(data.stream_id || "").trim();
  if (!streamId) return null;
  const pendingEventId = String(data.pending_event_id || "").trim();
  const actorId = String(event.by || "").trim();
  const existingIndex = bucket.streamingEvents.findIndex((item) => {
    const itemStreamId = String(
      (item.data as { stream_id?: unknown } | undefined)?.stream_id || "",
    ).trim();
    return itemStreamId === streamId;
  });
  const nextStreamingEvents = bucket.streamingEvents.slice();
  if (existingIndex >= 0) {
    nextStreamingEvents[existingIndex] = event;
  } else {
    nextStreamingEvents.push(event);
  }
  const patch: StreamingChatBucketPatch = { streamingEvents: nextStreamingEvents };
  if (pendingEventId && actorId) {
    const { replySessionsByPendingEventId, pendingEventIdByStreamId } = upsertReplySession(
      bucket.replySessionsByPendingEventId,
      bucket.pendingEventIdByStreamId,
      { pendingEventId, actorId, streamId, phase: event._streaming ? "streaming" : "pending" },
    );
    patch.replySessionsByPendingEventId = replySessionsByPendingEventId;
    patch.pendingEventIdByStreamId = pendingEventIdByStreamId;
  }
  return patch;
}

export function upsertStreamingTextPatch(
  bucket: StreamingChatBucket,
  streamId: string,
  text: string,
): StreamingChatBucketPatch | null {
  const targetStreamId = String(streamId || "").trim();
  if (!targetStreamId) return null;
  const previousText = String(bucket.streamingTextByStreamId[targetStreamId] || "");
  if (previousText === text) return null;
  const patch: StreamingChatBucketPatch = {
    streamingTextByStreamId: { ...bucket.streamingTextByStreamId, [targetStreamId]: text },
  };
  return patch;
}

export function upsertStreamingActivitiesPatch(
  bucket: StreamingChatBucket,
  streamId: string,
  activities: StreamingActivity[],
): StreamingChatBucketPatch | null {
  const targetStreamId = String(streamId || "").trim();
  if (!targetStreamId) return null;
  const nextActivities = normalizeStreamingActivityLog(activities);
  const previousActivities = bucket.streamingActivitiesByStreamId[targetStreamId] || [];
  if (JSON.stringify(previousActivities) === JSON.stringify(nextActivities)) return null;
  return {
    streamingActivitiesByStreamId: {
      ...bucket.streamingActivitiesByStreamId,
      [targetStreamId]: nextActivities,
    },
  };
}

export function upsertStreamingActivityPatch(
  bucket: StreamingChatBucket,
  groupId: string,
  actorId: string,
  match: { pendingEventId?: string; streamId?: string },
  activity: StreamingActivity,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  const pendingEventId = String(match.pendingEventId || "").trim();
  const streamId = String(match.streamId || "").trim();
  const activityId = String(activity.id || "").trim();
  const summary = String(activity.summary || "").trim();
  if (!targetActorId || !activityId || !summary) return null;

  const targetIndex = bucket.streamingEvents.findIndex((item) => {
    if (String(item.by || "").trim() !== targetActorId) return false;
    const data = item.data as
      | {
          pending_event_id?: unknown;
          pending_placeholder?: unknown;
          stream_id?: unknown;
          text?: unknown;
        }
      | undefined;
    const itemStreamId = String(data?.stream_id || "").trim();
    const itemPendingEventId = String(data?.pending_event_id || "").trim();
    if (streamId && itemStreamId === streamId) return true;
    if (!pendingEventId || itemPendingEventId !== pendingEventId) return false;

    const itemText = String(data?.text || "").trim();
    const isPendingPlaceholder = Boolean(data?.pending_placeholder);
    const isPendingProcessBubble = itemStreamId.startsWith("pending:") && !itemText;
    return isPendingPlaceholder || isPendingProcessBubble;
  });

  const nextStreamingEvents = bucket.streamingEvents.slice();
  const nextActivity: StreamingActivity = { ...activity, id: activityId, summary };
  let targetStreamId = streamId;
  let nextReplySessionsByPendingEventId = bucket.replySessionsByPendingEventId;
  let nextPendingEventIdByStreamId = bucket.pendingEventIdByStreamId;

  if (targetIndex < 0) {
    if (!pendingEventId) return null;
    targetStreamId = streamId || `pending:${pendingEventId}:${targetActorId}`;
    nextStreamingEvents.push({
      id: `pending:${pendingEventId}:${targetActorId}`,
      ts: nextActivity.ts || new Date().toISOString(),
      kind: "chat.message",
      group_id: groupId,
      by: targetActorId,
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: targetStreamId,
        pending_event_id: pendingEventId,
        pending_placeholder: !streamId,
        activities: [nextActivity],
      },
    });
    ({
      replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
      pendingEventIdByStreamId: nextPendingEventIdByStreamId,
    } = upsertReplySession(nextReplySessionsByPendingEventId, nextPendingEventIdByStreamId, {
      pendingEventId,
      actorId: targetActorId,
      streamId: targetStreamId,
      phase: streamId ? "streaming" : "pending",
      updatedAt: normalizeReplySessionTimestamp(nextActivity.ts),
    }));
    return {
      streamingEvents: nextStreamingEvents,
      streamingActivitiesByStreamId: {
        ...bucket.streamingActivitiesByStreamId,
        [targetStreamId]: [nextActivity],
      },
      replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
      pendingEventIdByStreamId: nextPendingEventIdByStreamId,
    };
  }

  const target = nextStreamingEvents[targetIndex];
  const data =
    target.data && typeof target.data === "object"
      ? (target.data as {
          activities?: StreamingActivity[];
          pending_placeholder?: unknown;
          stream_id?: unknown;
        })
      : {};
  targetStreamId =
    streamId || String(data.stream_id || "").trim() || `pending:${pendingEventId}:${targetActorId}`;
  const previousActivities = Array.isArray(data.activities) ? data.activities : [];
  const nextActivities = normalizeStreamingActivityLog(
    dedupeStreamingActivities(previousActivities.concat(nextActivity)).sort((left, right) =>
      String(left.ts || "").localeCompare(String(right.ts || "")),
    ),
  );
  nextStreamingEvents[targetIndex] = {
    ...target,
    ts: nextActivity.ts || target.ts,
    data: {
      ...data,
      stream_id: targetStreamId,
      pending_placeholder: streamId ? false : Boolean(data.pending_placeholder),
      activities: nextActivities,
    },
  };
  if (pendingEventId) {
    ({
      replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
      pendingEventIdByStreamId: nextPendingEventIdByStreamId,
    } = upsertReplySession(nextReplySessionsByPendingEventId, nextPendingEventIdByStreamId, {
      pendingEventId,
      actorId: targetActorId,
      streamId: targetStreamId,
      phase: streamId ? "streaming" : "pending",
      updatedAt: normalizeReplySessionTimestamp(nextActivity.ts),
    }));
  }
  return {
    streamingEvents: nextStreamingEvents,
    streamingActivitiesByStreamId: {
      ...bucket.streamingActivitiesByStreamId,
      [targetStreamId]: nextActivities,
    },
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function removeStreamingEventPatch(
  bucket: StreamingChatBucket,
  streamId: string,
): StreamingChatBucketPatch | null {
  const targetStreamId = String(streamId || "").trim();
  if (!targetStreamId) return null;
  const hadText = Object.prototype.hasOwnProperty.call(
    bucket.streamingTextByStreamId,
    targetStreamId,
  );
  const hadActivities = Object.prototype.hasOwnProperty.call(
    bucket.streamingActivitiesByStreamId,
    targetStreamId,
  );
  const hadPendingBinding = Object.prototype.hasOwnProperty.call(
    bucket.pendingEventIdByStreamId,
    targetStreamId,
  );
  const nextStreamingEvents = bucket.streamingEvents.filter((item) => {
    const itemStreamId = String(
      (item.data as { stream_id?: unknown } | undefined)?.stream_id || "",
    ).trim();
    return itemStreamId !== targetStreamId;
  });
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, [targetStreamId]);
  if (
    nextStreamingEvents.length === bucket.streamingEvents.length &&
    !hadText &&
    !hadActivities &&
    !hadPendingBinding
  ) {
    return null;
  }
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function removeStreamingEventsByPrefixPatch(
  bucket: StreamingChatBucket,
  streamIdPrefix: string,
): StreamingChatBucketPatch | null {
  const targetPrefix = String(streamIdPrefix || "").trim();
  if (!targetPrefix) return null;
  const removedStreamIds = bucket.streamingEvents
    .map((item) =>
      String((item.data as { stream_id?: unknown } | undefined)?.stream_id || "").trim(),
    )
    .filter((streamId) => streamId.startsWith(targetPrefix));
  if (removedStreamIds.length <= 0) return null;
  const removedSet = new Set(removedStreamIds);
  const nextStreamingEvents = bucket.streamingEvents.filter((item) => {
    const itemStreamId = String(
      (item.data as { stream_id?: unknown } | undefined)?.stream_id || "",
    ).trim();
    return !removedSet.has(itemStreamId);
  });
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, removedStreamIds);
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function promoteStreamingEventsByPrefixPatch(
  bucket: StreamingChatBucket,
  streamIdPrefix: string,
  pendingEventId: string,
): StreamingChatBucketPatch | null {
  const targetPrefix = String(streamIdPrefix || "").trim();
  const targetPendingEventId = String(pendingEventId || "").trim();
  if (!targetPrefix || !targetPendingEventId) return null;

  let changed = false;
  const renamedStreamIds = new Map<string, string>();
  const nextStreamingEvents = bucket.streamingEvents.map((item) => {
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as {
            stream_id?: unknown;
            pending_event_id?: unknown;
            pending_placeholder?: unknown;
          })
        : {};
    const streamId = String(data.stream_id || "").trim();
    if (!streamId.startsWith(targetPrefix)) return item;
    changed = true;
    const actorId = String(item.by || "").trim();
    const nextStreamId = `pending:${targetPendingEventId}:${actorId}`;
    renamedStreamIds.set(streamId, nextStreamId);
    return {
      ...item,
      data: {
        ...data,
        stream_id: nextStreamId,
        pending_event_id: targetPendingEventId,
        pending_placeholder: true,
      },
    };
  });
  if (!changed) return null;

  const nextStreamingTextByStreamId = { ...bucket.streamingTextByStreamId };
  const nextStreamingActivitiesByStreamId = { ...bucket.streamingActivitiesByStreamId };
  const nextPendingEventIdByStreamId = { ...bucket.pendingEventIdByStreamId };
  for (const streamId of Object.keys(bucket.streamingTextByStreamId || {})) {
    if (!streamId.startsWith(targetPrefix)) continue;
    const actorId = streamId.slice(targetPrefix.length).trim();
    if (!actorId) continue;
    const nextStreamId = `pending:${targetPendingEventId}:${actorId}`;
    nextStreamingTextByStreamId[nextStreamId] = nextStreamingTextByStreamId[streamId];
    delete nextStreamingTextByStreamId[streamId];
  }
  for (const streamId of Object.keys(bucket.streamingActivitiesByStreamId || {})) {
    if (!streamId.startsWith(targetPrefix)) continue;
    const actorId = streamId.slice(targetPrefix.length).trim();
    if (!actorId) continue;
    const nextStreamId = `pending:${targetPendingEventId}:${actorId}`;
    nextStreamingActivitiesByStreamId[nextStreamId] = nextStreamingActivitiesByStreamId[streamId];
    delete nextStreamingActivitiesByStreamId[streamId];
  }
  for (const [previousStreamId, nextStreamId] of renamedStreamIds.entries()) {
    delete nextPendingEventIdByStreamId[previousStreamId];
    nextPendingEventIdByStreamId[nextStreamId] = targetPendingEventId;
  }
  const previousPendingEventId =
    targetPrefix.startsWith("local:") && targetPrefix.endsWith(":")
      ? targetPrefix.slice("local:".length, -1)
      : "";
  const {
    replySessionsByPendingEventId: migratedReplySessionsByPendingEventId,
    pendingEventIdByStreamId: migratedPendingEventIdByStreamId,
  } = migrateReplySession(
    bucket.replySessionsByPendingEventId,
    nextPendingEventIdByStreamId,
    previousPendingEventId,
    targetPendingEventId,
  );
  const replySessionsByPendingEventId = { ...migratedReplySessionsByPendingEventId };
  const migratedSession = replySessionsByPendingEventId[targetPendingEventId];
  const migratedCurrentStreamId = String(migratedSession?.currentStreamId || "").trim();
  if (migratedSession && migratedCurrentStreamId && renamedStreamIds.has(migratedCurrentStreamId)) {
    replySessionsByPendingEventId[targetPendingEventId] = {
      ...migratedSession,
      currentStreamId: renamedStreamIds.get(migratedCurrentStreamId),
    };
  }

  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId,
    pendingEventIdByStreamId: migratedPendingEventIdByStreamId,
  };
}

export function promoteStreamingEventToStreamPatch(
  bucket: StreamingChatBucket,
  actorId: string,
  pendingEventId: string,
  streamId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  const targetPendingEventId = String(pendingEventId || "").trim();
  const targetStreamId = String(streamId || "").trim();
  if (!targetActorId || !targetPendingEventId || !targetStreamId) return null;

  const matchedIndex = bucket.streamingEvents.findIndex((item) => {
    if (String(item.by || "").trim() !== targetActorId) return false;
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as {
            pending_event_id?: unknown;
            pending_placeholder?: unknown;
            stream_id?: unknown;
            text?: unknown;
          })
        : {};
    if (String(data.stream_id || "").trim() === targetStreamId) return true;
    if (String(data.pending_event_id || "").trim() !== targetPendingEventId) return false;
    return isBindablePendingStream(data);
  });
  const targetIndex =
    matchedIndex >= 0
      ? matchedIndex
      : findLatestBindableLocalPlaceholderIndex(bucket.streamingEvents, targetActorId);
  if (targetIndex < 0) return null;

  const target = bucket.streamingEvents[targetIndex];
  const data =
    target.data && typeof target.data === "object"
      ? (target.data as {
          stream_id?: unknown;
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
        })
      : {};
  const previousStreamId = String(data.stream_id || "").trim();
  if (
    previousStreamId === targetStreamId &&
    !data.pending_placeholder &&
    String(data.pending_event_id || "").trim() === targetPendingEventId
  ) {
    return null;
  }

  const nextStreamingEvents = bucket.streamingEvents.slice();
  nextStreamingEvents[targetIndex] = {
    ...target,
    data: {
      ...data,
      stream_id: targetStreamId,
      pending_event_id: targetPendingEventId,
      pending_placeholder: false,
    },
  };

  const nextStreamingTextByStreamId = { ...bucket.streamingTextByStreamId };
  const nextStreamingActivitiesByStreamId = { ...bucket.streamingActivitiesByStreamId };
  if (
    previousStreamId &&
    previousStreamId !== targetStreamId &&
    Object.prototype.hasOwnProperty.call(nextStreamingTextByStreamId, previousStreamId)
  ) {
    nextStreamingTextByStreamId[targetStreamId] = nextStreamingTextByStreamId[previousStreamId];
    delete nextStreamingTextByStreamId[previousStreamId];
  }
  if (
    previousStreamId &&
    previousStreamId !== targetStreamId &&
    Object.prototype.hasOwnProperty.call(nextStreamingActivitiesByStreamId, previousStreamId)
  ) {
    nextStreamingActivitiesByStreamId[targetStreamId] =
      nextStreamingActivitiesByStreamId[previousStreamId];
    delete nextStreamingActivitiesByStreamId[previousStreamId];
  }
  const nextPendingEventIdByStreamId = { ...bucket.pendingEventIdByStreamId };
  if (previousStreamId && previousStreamId !== targetStreamId) {
    delete nextPendingEventIdByStreamId[previousStreamId];
  }
  nextPendingEventIdByStreamId[targetStreamId] = targetPendingEventId;
  const { replySessionsByPendingEventId, pendingEventIdByStreamId } = upsertReplySession(
    bucket.replySessionsByPendingEventId,
    nextPendingEventIdByStreamId,
    {
      pendingEventId: targetPendingEventId,
      actorId: targetActorId,
      streamId: targetStreamId,
      phase: "streaming",
    },
  );

  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId,
    pendingEventIdByStreamId,
  };
}

export function reconcileStreamingMessagePatch(
  bucket: StreamingChatBucket,
  groupId: string,
  actorId: string,
  args: {
    pendingEventId?: string;
    streamId: string;
    ts: string;
    fullText: string;
    eventText: string;
    activities: StreamingActivity[];
    completed: boolean;
    transientStream: boolean;
    phase?: string;
  },
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  const targetPendingEventId = String(args.pendingEventId || "").trim();
  const targetStreamId = String(args.streamId || "").trim();
  if (!targetActorId || !targetStreamId) return null;

  const nextPlaceholderState = !String(args.fullText || "").trim() && args.activities.length <= 0;
  const nextStreamingEvents = bucket.streamingEvents.slice();
  const matchedIndex = nextStreamingEvents.findIndex((item) => {
    if (String(item.by || "").trim() !== targetActorId) return false;
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as {
            pending_event_id?: unknown;
            pending_placeholder?: unknown;
            stream_id?: unknown;
            text?: unknown;
          })
        : {};
    if (String(data.stream_id || "").trim() === targetStreamId) return true;
    if (
      !targetPendingEventId ||
      String(data.pending_event_id || "").trim() !== targetPendingEventId
    )
      return false;
    return isBindablePendingStream(data);
  });
  const targetIndex =
    matchedIndex >= 0
      ? matchedIndex
      : findLatestBindableLocalPlaceholderIndex(nextStreamingEvents, targetActorId);

  let previousStreamId = "";
  const normalizedActivities = normalizeStreamingActivityLog(args.activities);
  let carriedActivities: StreamingActivity[] = [];
  if (targetIndex >= 0) {
    const target = nextStreamingEvents[targetIndex];
    const data =
      target.data && typeof target.data === "object"
        ? (target.data as {
            activities?: unknown;
            pending_event_id?: unknown;
            pending_placeholder?: unknown;
            text?: unknown;
            transient_stream?: unknown;
            stream_phase?: unknown;
            stream_id?: unknown;
          })
        : {};
    previousStreamId = String(data.stream_id || "").trim();
    carriedActivities = Array.isArray(data.activities)
      ? normalizeStreamingActivityLog(data.activities)
      : [];
    nextStreamingEvents[targetIndex] = {
      ...target,
      ts: args.ts || target.ts,
      _streaming: !args.completed,
      data: {
        ...data,
        text: args.eventText,
        to: Array.isArray((target.data as { to?: unknown } | undefined)?.to)
          ? (target.data as { to?: unknown[] }).to
          : ["user"],
        stream_id: targetStreamId,
        pending_event_id: targetPendingEventId || undefined,
        pending_placeholder: nextPlaceholderState,
        activities: normalizedActivities,
        transient_stream: args.transientStream,
        stream_phase: args.phase || undefined,
      },
    };
  } else {
    nextStreamingEvents.push({
      id: `stream:${targetStreamId}`,
      ts: args.ts,
      kind: "chat.message",
      group_id: groupId,
      by: targetActorId,
      _streaming: !args.completed,
      data: {
        text: args.eventText,
        to: ["user"],
        stream_id: targetStreamId,
        pending_event_id: targetPendingEventId || undefined,
        pending_placeholder: nextPlaceholderState,
        activities: normalizedActivities,
        transient_stream: args.transientStream,
        stream_phase: args.phase || undefined,
      },
    });
  }

  if (carriedActivities.length <= 0 && previousStreamId) {
    carriedActivities = normalizeStreamingActivityLog(
      bucket.streamingActivitiesByStreamId[previousStreamId] || [],
    );
  }

  const effectiveActivities =
    normalizedActivities.length > 0 ? normalizedActivities : carriedActivities;
  const resolvedPlaceholderState =
    !String(args.fullText || "").trim() && effectiveActivities.length <= 0;

  const targetEventIndex = nextStreamingEvents.findIndex((item) => {
    if (String(item.by || "").trim() !== targetActorId) return false;
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as { stream_id?: unknown; pending_event_id?: unknown })
        : {};
    return (
      String(data.stream_id || "").trim() === targetStreamId ||
      (!!targetPendingEventId &&
        String(data.pending_event_id || "").trim() === targetPendingEventId)
    );
  });
  if (targetEventIndex >= 0) {
    const target = nextStreamingEvents[targetEventIndex];
    const data =
      target.data && typeof target.data === "object"
        ? (target.data as Record<string, unknown>)
        : {};
    nextStreamingEvents[targetEventIndex] = {
      ...target,
      data: {
        ...data,
        pending_placeholder: resolvedPlaceholderState,
        activities: effectiveActivities,
      },
    };
  }

  const removedPlaceholderStreamIds: string[] = [];
  const dedupedStreamingEvents = nextStreamingEvents.filter((item) => {
    if (String(item.by || "").trim() !== targetActorId) return true;
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as {
            pending_event_id?: unknown;
            pending_placeholder?: unknown;
            stream_id?: unknown;
          })
        : {};
    const itemPendingEventId = String(data.pending_event_id || "").trim();
    const itemStreamId = String(data.stream_id || "").trim();
    const isPendingPlaceholder = Boolean(data.pending_placeholder);
    if (
      itemStreamId !== targetStreamId &&
      isPendingPlaceholder &&
      targetPendingEventId &&
      itemPendingEventId === targetPendingEventId
    ) {
      if (itemStreamId) removedPlaceholderStreamIds.push(itemStreamId);
      return false;
    }
    return true;
  });

  const nextStreamingTextByStreamId = { ...bucket.streamingTextByStreamId };
  const nextStreamingActivitiesByStreamId = { ...bucket.streamingActivitiesByStreamId };
  if (
    previousStreamId &&
    previousStreamId !== targetStreamId &&
    Object.prototype.hasOwnProperty.call(nextStreamingTextByStreamId, previousStreamId)
  ) {
    nextStreamingTextByStreamId[targetStreamId] = nextStreamingTextByStreamId[previousStreamId];
    delete nextStreamingTextByStreamId[previousStreamId];
  }
  nextStreamingTextByStreamId[targetStreamId] = args.fullText;

  if (
    previousStreamId &&
    previousStreamId !== targetStreamId &&
    Object.prototype.hasOwnProperty.call(nextStreamingActivitiesByStreamId, previousStreamId)
  ) {
    nextStreamingActivitiesByStreamId[targetStreamId] =
      nextStreamingActivitiesByStreamId[previousStreamId];
    delete nextStreamingActivitiesByStreamId[previousStreamId];
  }
  if (effectiveActivities.length > 0) {
    nextStreamingActivitiesByStreamId[targetStreamId] = effectiveActivities;
  } else {
    delete nextStreamingActivitiesByStreamId[targetStreamId];
  }

  for (const removedStreamId of removedPlaceholderStreamIds) {
    delete nextStreamingTextByStreamId[removedStreamId];
    delete nextStreamingActivitiesByStreamId[removedStreamId];
  }
  const nextPendingEventIdByStreamId = { ...bucket.pendingEventIdByStreamId };
  if (previousStreamId && previousStreamId !== targetStreamId) {
    delete nextPendingEventIdByStreamId[previousStreamId];
  }
  if (targetPendingEventId) {
    nextPendingEventIdByStreamId[targetStreamId] = targetPendingEventId;
  } else {
    delete nextPendingEventIdByStreamId[targetStreamId];
  }
  for (const removedStreamId of removedPlaceholderStreamIds) {
    delete nextPendingEventIdByStreamId[removedStreamId];
  }
  const { replySessionsByPendingEventId, pendingEventIdByStreamId } = targetPendingEventId
    ? upsertReplySession(bucket.replySessionsByPendingEventId, nextPendingEventIdByStreamId, {
        pendingEventId: targetPendingEventId,
        actorId: targetActorId,
        streamId: targetStreamId,
        phase: resolvedPlaceholderState ? "pending" : "streaming",
        updatedAt: normalizeReplySessionTimestamp(args.ts),
      })
    : {
        replySessionsByPendingEventId: bucket.replySessionsByPendingEventId,
        pendingEventIdByStreamId: nextPendingEventIdByStreamId,
      };

  return {
    streamingEvents: dedupedStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId,
    pendingEventIdByStreamId,
  };
}

export function completeStreamingEventsForActorPatch(
  bucket: StreamingChatBucket,
  actorId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  if (!targetActorId) return null;
  let changed = false;
  let nextReplySessionsByPendingEventId = bucket.replySessionsByPendingEventId;
  let nextPendingEventIdByStreamId = bucket.pendingEventIdByStreamId;
  const nextStreamingEvents = bucket.streamingEvents.map((item) => {
    if (String(item.by || "").trim() !== targetActorId) return item;
    const data =
      item.data && typeof item.data === "object"
        ? (item.data as {
            pending_placeholder?: unknown;
            pending_event_id?: unknown;
            stream_id?: unknown;
            text?: unknown;
            activities?: unknown;
          })
        : {};
    const pendingEventId = String(data.pending_event_id || "").trim();
    const streamId = String(data.stream_id || "").trim();
    const text = String(data.text || "").trim();
    const activities = Array.isArray(data.activities) ? data.activities : [];
    const queuedOnly = isQueuedOnlyActivityList(activities);
    const isFreshLocalPlaceholder =
      (!pendingEventId || pendingEventId.startsWith("local_")) &&
      streamId.startsWith("local:") &&
      !text &&
      queuedOnly;
    if (isFreshLocalPlaceholder) return item;
    if (pendingEventId) {
      changed = true;
      ({
        replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
        pendingEventIdByStreamId: nextPendingEventIdByStreamId,
      } = upsertReplySession(nextReplySessionsByPendingEventId, nextPendingEventIdByStreamId, {
        pendingEventId,
        actorId: targetActorId,
        streamId,
        phase: "completed",
      }));
    }
    if (!item._streaming) return item;
    changed = true;
    return { ...item, _streaming: false, data: { ...data, pending_placeholder: false } };
  });
  if (!changed) return null;
  return {
    streamingEvents: nextStreamingEvents,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function clearStreamingEventsForActorPatch(
  bucket: StreamingChatBucket,
  actorId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  if (!targetActorId) return null;
  const nextStreamingEvents = bucket.streamingEvents.filter(
    (item) => String(item.by || "").trim() !== targetActorId,
  );
  const removedStreamIds = bucket.streamingEvents
    .filter((item) => String(item.by || "").trim() === targetActorId)
    .map((item) =>
      String((item.data as { stream_id?: unknown } | undefined)?.stream_id || "").trim(),
    )
    .filter(Boolean);
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, removedStreamIds);
  if (nextStreamingEvents.length === bucket.streamingEvents.length) return null;
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function clearEmptyStreamingEventsForActorPatch(
  bucket: StreamingChatBucket,
  actorId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  if (!targetActorId) return null;
  const removedStreamIds = bucket.streamingEvents
    .filter((item) => {
      if (String(item.by || "").trim() !== targetActorId) return false;
      const data =
        item.data && typeof item.data === "object"
          ? (item.data as {
              text?: unknown;
              stream_id?: unknown;
              activities?: unknown;
              pending_event_id?: unknown;
            })
          : {};
      const streamId = String(data.stream_id || "").trim();
      const pendingEventId = String(data.pending_event_id || "").trim();
      const eventText = String(data.text || "").trim();
      const cachedText = streamId
        ? String(bucket.streamingTextByStreamId[streamId] || "").trim()
        : "";
      if (eventText || cachedText) return false;
      if (!pendingEventId && streamId.startsWith("local:")) {
        return false;
      }
      // Once a canonical reply exists, only queued-only placeholders are stale.
      // Real activity timelines still represent in-flight or just-finished work.
      if (
        pendingEventId &&
        hasCanonicalReplyForPendingEvent(bucket, targetActorId, pendingEventId)
      ) {
        return isQueuedOnlyActivityList(data.activities);
      }
      if (!isQueuedOnlyActivityList(data.activities)) return false;
      // No canonical reply yet — keep waiting.
      if (pendingEventId) return false;
      return true;
    })
    .map((item) =>
      String((item.data as { stream_id?: unknown } | undefined)?.stream_id || "").trim(),
    )
    .filter(Boolean);
  if (removedStreamIds.length === 0) return null;
  const removedSet = new Set(removedStreamIds);
  const nextStreamingEvents = bucket.streamingEvents.filter((item) => {
    const itemStreamId = String(
      (item.data as { stream_id?: unknown } | undefined)?.stream_id || "",
    ).trim();
    return !removedSet.has(itemStreamId);
  });
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, removedStreamIds);
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function clearTransientStreamingEventsForActorPatch(
  bucket: StreamingChatBucket,
  actorId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  if (!targetActorId) return null;
  const removedStreamIds = bucket.streamingEvents
    .filter((item) => {
      if (String(item.by || "").trim() !== targetActorId) return false;
      const data =
        item.data && typeof item.data === "object"
          ? (item.data as {
              transient_stream?: unknown;
              stream_id?: unknown;
              pending_event_id?: unknown;
            })
          : {};
      if (!data.transient_stream) return false;
      const streamId = String(data.stream_id || "").trim();
      const pendingEventId = String(data.pending_event_id || "").trim();
      if (!pendingEventId) return true;
      if (hasCanonicalReplyForPendingEvent(bucket, targetActorId, pendingEventId)) return true;
      return bucket.streamingEvents.some((candidate) => {
        if (candidate === item) return false;
        if (String(candidate.by || "").trim() !== targetActorId) return false;
        const candidateData =
          candidate.data && typeof candidate.data === "object"
            ? (candidate.data as {
                stream_id?: unknown;
                pending_event_id?: unknown;
                transient_stream?: unknown;
              })
            : {};
        if (candidateData.transient_stream) return false;
        if (String(candidateData.pending_event_id || "").trim() !== pendingEventId) return false;
        return String(candidateData.stream_id || "").trim() !== streamId;
      });
    })
    .map((item) =>
      String((item.data as { stream_id?: unknown } | undefined)?.stream_id || "").trim(),
    )
    .filter(Boolean);
  if (removedStreamIds.length === 0) return null;
  const removedSet = new Set(removedStreamIds);
  const nextStreamingEvents = bucket.streamingEvents.filter((item) => {
    const itemStreamId = String(
      (item.data as { stream_id?: unknown } | undefined)?.stream_id || "",
    ).trim();
    return !removedSet.has(itemStreamId);
  });
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, removedStreamIds);
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function clearStreamingPlaceholderPatch(
  bucket: StreamingChatBucket,
  actorId: string,
  pendingEventId: string,
): StreamingChatBucketPatch | null {
  const targetActorId = String(actorId || "").trim();
  const targetPendingEventId = String(pendingEventId || "").trim();
  if (!targetActorId || !targetPendingEventId) return null;
  const removedStreamIds = bucket.streamingEvents
    .filter((item) => {
      if (String(item.by || "").trim() !== targetActorId) return false;
      const data = item.data as
        | { pending_event_id?: unknown; pending_placeholder?: unknown; stream_id?: unknown }
        | undefined;
      const itemPendingEventId = String(data?.pending_event_id || "").trim();
      const isPendingPlaceholder = Boolean(data?.pending_placeholder);
      return isPendingPlaceholder && itemPendingEventId === targetPendingEventId;
    })
    .map((item) =>
      String((item.data as { stream_id?: unknown } | undefined)?.stream_id || "").trim(),
    )
    .filter(Boolean);
  const nextStreamingEvents = bucket.streamingEvents.filter((item) => {
    if (String(item.by || "").trim() !== targetActorId) return true;
    const data = item.data as
      | { pending_event_id?: unknown; pending_placeholder?: unknown }
      | undefined;
    const itemPendingEventId = String(data?.pending_event_id || "").trim();
    const isPendingPlaceholder = Boolean(data?.pending_placeholder);
    return !(isPendingPlaceholder && itemPendingEventId === targetPendingEventId);
  });
  if (nextStreamingEvents.length === bucket.streamingEvents.length) return null;
  const {
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  } = removeStreamBindings(bucket, removedStreamIds);
  return {
    streamingEvents: nextStreamingEvents,
    streamingTextByStreamId: nextStreamingTextByStreamId,
    streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
    replySessionsByPendingEventId: {
      ...nextReplySessionsByPendingEventId,
      [targetPendingEventId]: {
        ...(nextReplySessionsByPendingEventId[targetPendingEventId] || {
          pendingEventId: targetPendingEventId,
          actorId: targetActorId,
          phase: "pending" as const,
          updatedAt: Date.now(),
        }),
        phase: "streaming",
        updatedAt: Date.now(),
      },
    },
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}
