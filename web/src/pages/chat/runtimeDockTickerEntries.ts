import { dedupeStreamingActivities } from "../../stores/chatStreamingSessions";
import type { HeadlessPreviewBlock, HeadlessPreviewSession, StreamingActivity } from "../../types";
import type { LiveWorkCard } from "./liveWorkCards";
import type { RuntimeDockItem } from "./runtimeDockItems";

export type RuntimeDockTickerEntry = {
  id: string;
  kind: "message" | "activity";
  actorId: string;
  actorLabel: string;
  text: string;
  updatedAt: string;
  sourceId?: string;
  completed?: boolean;
};

const TICKER_ENTRY_LIMIT = 80;

function isLiveWorkCardActive(card: LiveWorkCard | null | undefined): boolean {
  if (!card) return false;
  return card.phase === "pending" || card.phase === "streaming";
}

function hasTickerTranscript(previewSessions: HeadlessPreviewSession[]): boolean {
  return previewSessions.some(
    (session) =>
      Array.isArray(session.transcriptBlocks) &&
      session.transcriptBlocks.some((block) => normalizeTickerText(block?.text)),
  );
}

function shouldIncludeTickerPreview(
  card: LiveWorkCard,
  previewSessions: HeadlessPreviewSession[],
): boolean {
  if (isLiveWorkCardActive(card)) return true;
  if (card.phase !== "completed" && card.phase !== "failed") return false;
  return hasTickerTranscript(previewSessions);
}

function isSubstantiveActivity(activity: StreamingActivity): boolean {
  const summary = String(activity.summary || "").trim();
  const kind = String(activity.kind || "")
    .trim()
    .toLowerCase();
  return Boolean(summary) && !(kind === "queued" && summary.toLowerCase() === "queued");
}

function getActivityTimestamp(activity: StreamingActivity, fallback: string): string {
  return String(activity.ts || fallback || "").trim();
}

function normalizeTickerText(value: unknown): string {
  return String(value || "")
    .replace(/\r\n/g, "\n")
    .trim();
}

function getPreviewSessionKey(session: HeadlessPreviewSession, fallback: string): string {
  return String(session.pendingEventId || session.currentStreamId || fallback || "").trim();
}

function getTickerPreviewSessions(
  item: RuntimeDockItem,
  card: LiveWorkCard,
): HeadlessPreviewSession[] {
  const previewSessions = Array.isArray(card.previewSessions)
    ? card.previewSessions.filter(Boolean)
    : [];
  if (previewSessions.length > 0) return previewSessions;

  const text = normalizeTickerText(card.text);
  const transcriptBlocks = Array.isArray(card.transcriptBlocks)
    ? card.transcriptBlocks.filter((block) => normalizeTickerText(block?.text))
    : [];
  const activities = Array.isArray(card.activities) ? card.activities.filter(Boolean) : [];
  if (!text && transcriptBlocks.length <= 0 && activities.length <= 0) return [];

  const sessionKey = String(card.pendingEventId || card.streamId || item.actorId || "").trim();
  const fallbackBlock =
    text && transcriptBlocks.length <= 0
      ? [
          {
            id: "latest",
            streamId: String(card.streamId || sessionKey || "").trim(),
            streamPhase: String(card.streamPhase || "")
              .trim()
              .toLowerCase(),
            text,
            updatedAt: String(card.updatedAt || "").trim(),
            completed: card.phase === "completed",
            transient: card.phase !== "completed",
          } satisfies HeadlessPreviewBlock,
        ]
      : [];

  return [
    {
      actorId: item.actorId,
      pendingEventId: sessionKey || item.actorId,
      currentStreamId: String(card.streamId || sessionKey || "").trim(),
      phase: card.phase,
      streamPhase: String(card.streamPhase || "")
        .trim()
        .toLowerCase(),
      updatedAt: String(card.updatedAt || "").trim(),
      latestText: text,
      transcriptBlocks: transcriptBlocks.length > 0 ? transcriptBlocks : fallbackBlock,
      activities,
    },
  ];
}

function buildMessageEntry(args: {
  item: RuntimeDockItem;
  session: HeadlessPreviewSession;
  block: HeadlessPreviewBlock;
  sessionKey: string;
}): RuntimeDockTickerEntry | null {
  const text = normalizeTickerText(args.block.text);
  if (!text) return null;
  const sourceId = ["message", args.item.actorId, args.sessionKey].join(":");
  const updatedAt = String(args.block.updatedAt || args.session.updatedAt || "").trim();
  return {
    id: sourceId,
    kind: "message",
    actorId: args.item.actorId,
    actorLabel: args.item.actorLabel,
    text,
    updatedAt,
    sourceId,
    completed: Boolean(args.block.completed),
  };
}

function getLatestTranscriptBlock(session: HeadlessPreviewSession): HeadlessPreviewBlock | null {
  const blocks = Array.isArray(session.transcriptBlocks) ? session.transcriptBlocks : [];
  let latest: HeadlessPreviewBlock | null = null;
  for (const block of blocks) {
    if (!normalizeTickerText(block?.text)) continue;
    if (!latest) {
      latest = block;
      continue;
    }
    const latestTs = String(latest.updatedAt || "").trim();
    const currentTs = String(block.updatedAt || "").trim();
    if (currentTs && (!latestTs || currentTs >= latestTs)) {
      latest = block;
    }
  }
  return latest;
}

function buildActivityEntry(args: {
  item: RuntimeDockItem;
  session: HeadlessPreviewSession;
  activity: StreamingActivity;
  sessionKey: string;
}): RuntimeDockTickerEntry | null {
  if (!isSubstantiveActivity(args.activity)) return null;
  const text = normalizeTickerText(args.activity.summary);
  if (!text) return null;
  const activityId = String(args.activity.id || args.activity.kind || text || "").trim();
  return {
    id: ["activity", args.item.actorId, args.sessionKey, activityId].join(":"),
    kind: "activity",
    actorId: args.item.actorId,
    actorLabel: args.item.actorLabel,
    text,
    updatedAt: getActivityTimestamp(args.activity, String(args.session.updatedAt || "").trim()),
  };
}

function compareTickerEntriesDescending(
  left: RuntimeDockTickerEntry,
  right: RuntimeDockTickerEntry,
): number {
  const leftTs = String(left.updatedAt || "").trim();
  const rightTs = String(right.updatedAt || "").trim();
  if (leftTs && rightTs && leftTs !== rightTs) return rightTs.localeCompare(leftTs);
  if (leftTs && !rightTs) return -1;
  if (!leftTs && rightTs) return 1;
  return right.id.localeCompare(left.id);
}

export function buildRuntimeDockTickerEntries(
  items: RuntimeDockItem[],
  limit = TICKER_ENTRY_LIMIT,
): RuntimeDockTickerEntry[] {
  const entries: RuntimeDockTickerEntry[] = [];
  const seen = new Set<string>();
  for (const item of Array.isArray(items) ? items : []) {
    const card = item.liveWorkCard;
    if (!card) continue;
    const previewSessions = getTickerPreviewSessions(item, card);
    if (!shouldIncludeTickerPreview(card, previewSessions)) continue;
    const includeActivities = isLiveWorkCardActive(card);
    for (const session of previewSessions) {
      const sessionKey = getPreviewSessionKey(
        session,
        card.pendingEventId || card.streamId || item.actorId,
      );
      const latestBlock = getLatestTranscriptBlock(session);
      const messageEntry = latestBlock
        ? buildMessageEntry({ item, session, block: latestBlock, sessionKey })
        : null;
      if (messageEntry && !seen.has(messageEntry.id)) {
        seen.add(messageEntry.id);
        entries.push(messageEntry);
      }

      if (!includeActivities) continue;
      for (const activity of dedupeStreamingActivities(session.activities || [])) {
        const entry = buildActivityEntry({ item, session, activity, sessionKey });
        if (!entry || seen.has(entry.id)) continue;
        seen.add(entry.id);
        entries.push(entry);
      }
    }
  }

  return entries.sort(compareTickerEntriesDescending).slice(0, Math.max(0, limit));
}
