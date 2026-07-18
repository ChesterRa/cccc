import type { StreamingActivity } from "../types";

export const STREAMING_ACTIVITY_LOG_LIMIT = 12;
const STREAMING_REPLY_SESSION_LIMIT = 80;

export type StreamingReplySessionPhase = "pending" | "streaming" | "completed" | "failed";

export type StreamingReplySession = {
  pendingEventId: string;
  actorId: string;
  currentStreamId?: string;
  canonicalEventId?: string;
  phase: StreamingReplySessionPhase;
  updatedAt: number;
};

function isTerminalReplySessionPhase(phase: unknown): boolean {
  const value = String(phase || "").trim();
  return value === "completed" || value === "failed";
}

function normalizeActivityText(value: unknown): string {
  return String(value || "").trim();
}

function normalizeActivityStringList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const normalized: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const text = normalizeActivityText(item);
    if (!text || seen.has(text)) continue;
    seen.add(text);
    normalized.push(text);
  }
  return normalized.length > 0 ? normalized : undefined;
}

export function mergeStreamingActivity(
  previous: StreamingActivity | undefined,
  next: StreamingActivity | undefined,
): StreamingActivity | null {
  const previousId = normalizeActivityText(previous?.id);
  const nextId = normalizeActivityText(next?.id);
  const activityId = nextId || previousId;
  const previousSummary = normalizeActivityText(previous?.summary);
  const nextSummary = normalizeActivityText(next?.summary);
  const summary = nextSummary || previousSummary;
  if (!activityId || !summary) return null;

  const previousFilePaths = normalizeActivityStringList(previous?.file_paths);
  const nextFilePaths = normalizeActivityStringList(next?.file_paths);
  const mergedFilePaths =
    previousFilePaths || nextFilePaths
      ? normalizeActivityStringList([...(previousFilePaths || []), ...(nextFilePaths || [])])
      : undefined;

  return {
    id: activityId,
    kind: normalizeActivityText(next?.kind) || normalizeActivityText(previous?.kind) || "thinking",
    status:
      normalizeActivityText(next?.status) || normalizeActivityText(previous?.status) || "updated",
    summary,
    detail:
      normalizeActivityText(next?.detail) || normalizeActivityText(previous?.detail) || undefined,
    ts: normalizeActivityText(previous?.ts) || normalizeActivityText(next?.ts) || undefined,
    raw_item_type:
      normalizeActivityText(next?.raw_item_type) ||
      normalizeActivityText(previous?.raw_item_type) ||
      undefined,
    tool_name:
      normalizeActivityText(next?.tool_name) ||
      normalizeActivityText(previous?.tool_name) ||
      undefined,
    server_name:
      normalizeActivityText(next?.server_name) ||
      normalizeActivityText(previous?.server_name) ||
      undefined,
    command:
      normalizeActivityText(next?.command) || normalizeActivityText(previous?.command) || undefined,
    cwd: normalizeActivityText(next?.cwd) || normalizeActivityText(previous?.cwd) || undefined,
    file_paths: mergedFilePaths,
    query:
      normalizeActivityText(next?.query) || normalizeActivityText(previous?.query) || undefined,
  };
}

export function dedupeStreamingActivities(
  activities: StreamingActivity[] | undefined,
): StreamingActivity[] {
  if (!Array.isArray(activities) || activities.length <= 0) return [];

  const activityOrder: string[] = [];
  const mergedById = new Map<string, StreamingActivity>();
  for (const activity of activities) {
    if (!activity || typeof activity !== "object") continue;
    const normalized = mergeStreamingActivity(undefined, activity);
    if (!normalized) continue;
    const existing = mergedById.get(normalized.id);
    if (!existing) {
      activityOrder.push(normalized.id);
      mergedById.set(normalized.id, normalized);
      continue;
    }
    const merged = mergeStreamingActivity(existing, normalized);
    if (merged) {
      mergedById.set(normalized.id, merged);
    }
  }

  return activityOrder
    .map((activityId) => mergedById.get(activityId))
    .filter((activity): activity is StreamingActivity => Boolean(activity));
}

export function sliceStreamingActivities(
  activities: StreamingActivity[] | undefined,
): StreamingActivity[] {
  return dedupeStreamingActivities(activities).slice(-STREAMING_ACTIVITY_LOG_LIMIT);
}

export function normalizeStreamingActivityLog(
  activities: StreamingActivity[] | undefined,
): StreamingActivity[] {
  return dedupeStreamingActivities(activities);
}

export function normalizeReplySessionTimestamp(ts?: string): number {
  const ms = Date.parse(String(ts || ""));
  return Number.isFinite(ms) ? ms : Date.now();
}

export function pruneReplySessions(
  sessions: Record<string, StreamingReplySession>,
  pendingEventIdByStreamId: Record<string, string>,
): {
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
} {
  const values = Object.values(sessions);
  if (values.length <= STREAMING_REPLY_SESSION_LIMIT) {
    return { replySessionsByPendingEventId: sessions, pendingEventIdByStreamId };
  }

  const removable = values
    .filter((session) => session.phase === "completed" || session.phase === "failed")
    .sort((left, right) => left.updatedAt - right.updatedAt);
  if (removable.length <= 0) {
    return { replySessionsByPendingEventId: sessions, pendingEventIdByStreamId };
  }

  const overflow = Math.max(0, values.length - STREAMING_REPLY_SESSION_LIMIT);
  const nextSessions = { ...sessions };
  const nextPendingEventIdByStreamId = { ...pendingEventIdByStreamId };
  for (const session of removable.slice(0, overflow)) {
    delete nextSessions[session.pendingEventId];
    for (const streamId of Object.keys(nextPendingEventIdByStreamId)) {
      if (nextPendingEventIdByStreamId[streamId] === session.pendingEventId) {
        delete nextPendingEventIdByStreamId[streamId];
      }
    }
  }

  return {
    replySessionsByPendingEventId: nextSessions,
    pendingEventIdByStreamId: nextPendingEventIdByStreamId,
  };
}

export function upsertReplySession(
  replySessionsByPendingEventId: Record<string, StreamingReplySession>,
  pendingEventIdByStreamId: Record<string, string>,
  args: {
    pendingEventId: string;
    actorId: string;
    streamId?: string;
    phase?: StreamingReplySessionPhase;
    canonicalEventId?: string;
    updatedAt?: number;
  },
): {
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
} {
  const pendingEventId = String(args.pendingEventId || "").trim();
  const actorId = String(args.actorId || "").trim();
  const streamId = String(args.streamId || "").trim();
  if (!pendingEventId || !actorId) {
    return { replySessionsByPendingEventId, pendingEventIdByStreamId };
  }

  const existing = replySessionsByPendingEventId[pendingEventId];
  const requestedPhase = args.phase || existing?.phase || "pending";
  const keepTerminalSession =
    Boolean(existing) &&
    isTerminalReplySessionPhase(existing?.phase) &&
    !isTerminalReplySessionPhase(requestedPhase);
  const nextPhase = keepTerminalSession ? existing?.phase || "completed" : requestedPhase;
  const nextSession: StreamingReplySession = {
    pendingEventId,
    actorId,
    currentStreamId: keepTerminalSession
      ? existing?.currentStreamId || streamId
      : streamId || existing?.currentStreamId,
    canonicalEventId: args.canonicalEventId || existing?.canonicalEventId,
    phase: nextPhase,
    updatedAt: keepTerminalSession
      ? Number(existing?.updatedAt || 0)
      : Number.isFinite(Number(args.updatedAt))
        ? Number(args.updatedAt)
        : Date.now(),
  };

  const nextReplySessionsByPendingEventId = {
    ...replySessionsByPendingEventId,
    [pendingEventId]: nextSession,
  };
  const nextPendingEventIdByStreamId = { ...pendingEventIdByStreamId };
  if (streamId) {
    nextPendingEventIdByStreamId[streamId] = pendingEventId;
  }
  return pruneReplySessions(nextReplySessionsByPendingEventId, nextPendingEventIdByStreamId);
}

export function migrateReplySession(
  replySessionsByPendingEventId: Record<string, StreamingReplySession>,
  pendingEventIdByStreamId: Record<string, string>,
  previousPendingEventId: string,
  nextPendingEventId: string,
): {
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
} {
  const fromKey = String(previousPendingEventId || "").trim();
  const toKey = String(nextPendingEventId || "").trim();
  if (!fromKey || !toKey || fromKey === toKey) {
    return { replySessionsByPendingEventId, pendingEventIdByStreamId };
  }

  const source = replySessionsByPendingEventId[fromKey];
  if (!source) {
    return { replySessionsByPendingEventId, pendingEventIdByStreamId };
  }

  const target = replySessionsByPendingEventId[toKey];
  const merged: StreamingReplySession = {
    pendingEventId: toKey,
    actorId: target?.actorId || source.actorId,
    currentStreamId: target?.currentStreamId || source.currentStreamId,
    canonicalEventId: target?.canonicalEventId || source.canonicalEventId,
    phase: target?.phase || source.phase,
    updatedAt: Math.max(target?.updatedAt || 0, source.updatedAt || 0, Date.now()),
  };

  const nextReplySessionsByPendingEventId = { ...replySessionsByPendingEventId };
  delete nextReplySessionsByPendingEventId[fromKey];
  nextReplySessionsByPendingEventId[toKey] = merged;
  const nextPendingEventIdByStreamId = { ...pendingEventIdByStreamId };
  for (const streamId of Object.keys(nextPendingEventIdByStreamId)) {
    if (nextPendingEventIdByStreamId[streamId] === fromKey) {
      nextPendingEventIdByStreamId[streamId] = toKey;
    }
  }
  return pruneReplySessions(nextReplySessionsByPendingEventId, nextPendingEventIdByStreamId);
}
