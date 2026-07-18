import { dedupeStreamingActivities } from "../../stores/chatStreamingSessions";
import type { HeadlessPreviewSession, StreamingActivity } from "../../types";

export type HeadlessPreviewTimelineEntry = {
  id: string;
  pendingEventId: string;
  ts: string;
  live: boolean;
} & (
  | { kind: "message"; streamPhase: string; text: string; completed: boolean; transient: boolean }
  | { kind: "activity"; activity: StreamingActivity }
);

export type HeadlessPreviewRenderGroup =
  | {
      id: string;
      kind: "message";
      pendingEventId: string;
      ts: string;
      live: boolean;
      entry: Extract<HeadlessPreviewTimelineEntry, { kind: "message" }>;
    }
  | {
      id: string;
      kind: "activity-band";
      pendingEventId: string;
      ts: string;
      live: boolean;
      entries: Array<Extract<HeadlessPreviewTimelineEntry, { kind: "activity" }>>;
    };

type BuildHeadlessPreviewTimelineArgs = {
  previewSessions?: HeadlessPreviewSession[];
  fallbackText?: string;
  fallbackActivities?: StreamingActivity[];
  fallbackUpdatedAt?: string;
  fallbackPendingEventId?: string;
  fallbackStreamId?: string;
  fallbackStreamPhase?: string;
  fallbackPhase?: string;
};

type SortableTimelineEntry = HeadlessPreviewTimelineEntry & { order: number };

function normalizeTimelineSessions(
  args: BuildHeadlessPreviewTimelineArgs,
): HeadlessPreviewSession[] {
  const previewSessions = Array.isArray(args.previewSessions)
    ? args.previewSessions.filter((session) => {
        if (!session) return false;
        const hasTranscriptBlocks =
          Array.isArray(session.transcriptBlocks) &&
          session.transcriptBlocks.some((block) => String(block?.text || "").trim());
        const hasActivities =
          Array.isArray(session.activities) &&
          session.activities.some((activity) => String(activity?.summary || "").trim());
        return (
          hasTranscriptBlocks || hasActivities || Boolean(String(session.latestText || "").trim())
        );
      })
    : [];

  if (previewSessions.length > 0) return previewSessions;

  const fallbackText = String(args.fallbackText || "").trim();
  const fallbackActivities = dedupeStreamingActivities(args.fallbackActivities || []).filter(
    (activity) => String(activity.summary || "").trim(),
  );
  if (!fallbackText && fallbackActivities.length <= 0) return [];

  const updatedAt = String(args.fallbackUpdatedAt || "").trim();
  const fallbackPhase = String(args.fallbackPhase || "")
    .trim()
    .toLowerCase();
  return [
    {
      actorId: "",
      pendingEventId:
        String(args.fallbackPendingEventId || "fallback-preview").trim() || "fallback-preview",
      currentStreamId:
        String(args.fallbackStreamId || "fallback-stream").trim() || "fallback-stream",
      phase: fallbackPhase || (fallbackText ? "streaming" : "pending"),
      streamPhase: String(args.fallbackStreamPhase || "")
        .trim()
        .toLowerCase(),
      updatedAt,
      latestText: fallbackText,
      transcriptBlocks: fallbackText
        ? [
            {
              id: `fallback:${String(args.fallbackPendingEventId || "preview").trim() || "preview"}`,
              streamId:
                String(args.fallbackStreamId || "fallback-stream").trim() || "fallback-stream",
              streamPhase: String(args.fallbackStreamPhase || "")
                .trim()
                .toLowerCase(),
              text: fallbackText,
              updatedAt,
              completed: false,
              transient:
                String(args.fallbackStreamPhase || "")
                  .trim()
                  .toLowerCase() === "commentary",
            },
          ]
        : [],
      activities: fallbackActivities,
    },
  ];
}

function compareTimelineEntries(left: SortableTimelineEntry, right: SortableTimelineEntry): number {
  const leftTs = String(left.ts || "").trim();
  const rightTs = String(right.ts || "").trim();
  if (leftTs && rightTs && leftTs !== rightTs) return leftTs.localeCompare(rightTs);
  if (leftTs && !rightTs) return 1;
  if (!leftTs && rightTs) return -1;
  if (left.pendingEventId && right.pendingEventId && left.pendingEventId !== right.pendingEventId) {
    return left.pendingEventId.localeCompare(right.pendingEventId);
  }
  if (left.kind !== right.kind) {
    return left.kind === "activity" ? -1 : 1;
  }
  return left.order - right.order;
}

function normalizeComparableText(value: string | undefined): string {
  return String(value || "")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

function isPlainReasoningActivity(activity: StreamingActivity): boolean {
  const kind = String(activity?.kind || "")
    .trim()
    .toLowerCase();
  const rawItemType = String(activity?.raw_item_type || "")
    .trim()
    .toLowerCase();
  const hasActionMetadata = Boolean(
    String(activity?.tool_name || "").trim() ||
    String(activity?.server_name || "").trim() ||
    String(activity?.command || "").trim() ||
    String(activity?.cwd || "").trim() ||
    String(activity?.query || "").trim() ||
    (Array.isArray(activity?.file_paths) &&
      activity.file_paths.some((path) => String(path || "").trim())),
  );
  if (hasActionMetadata) return false;
  return (
    rawItemType === "reasoning" ||
    (kind === "thinking" && (!rawItemType || rawItemType === "reasoning"))
  );
}

function duplicatesTranscriptText(activity: StreamingActivity, transcriptTexts: string[]): boolean {
  if (!isPlainReasoningActivity(activity)) return false;
  const summary = normalizeComparableText(activity?.summary);
  if (summary.length < 8) return false;
  return transcriptTexts.some((text) => {
    if (text.length < 8) return false;
    return text === summary || text.startsWith(summary) || summary.startsWith(text);
  });
}

export function buildHeadlessPreviewTimelineEntries(
  args: BuildHeadlessPreviewTimelineArgs,
): HeadlessPreviewTimelineEntry[] {
  const previewSessions = normalizeTimelineSessions(args);
  if (previewSessions.length <= 0) return [];

  const latestPendingEventId = String(
    previewSessions[previewSessions.length - 1]?.pendingEventId || "",
  ).trim();
  let order = 0;
  const entries: SortableTimelineEntry[] = [];

  for (const session of previewSessions) {
    const pendingEventId = String(session.pendingEventId || "").trim();
    const liveSession =
      latestPendingEventId === pendingEventId &&
      !["completed", "failed"].includes(
        String(session.phase || "")
          .trim()
          .toLowerCase(),
      );
    const narrativeTranscriptTexts = (
      Array.isArray(session.transcriptBlocks) ? session.transcriptBlocks : []
    )
      .filter(
        (block) =>
          String(block?.streamPhase || "")
            .trim()
            .toLowerCase() !== "final_answer",
      )
      .map((block) => normalizeComparableText(block?.text))
      .filter(Boolean);

    for (const block of Array.isArray(session.transcriptBlocks) ? session.transcriptBlocks : []) {
      const text = String(block?.text || "").trim();
      if (!text) continue;
      entries.push({
        id: `message:${pendingEventId}:${String(block.id || "").trim() || order}`,
        kind: "message",
        pendingEventId,
        ts: String(block.updatedAt || session.updatedAt || "").trim(),
        live: liveSession && !block.completed,
        streamPhase: String(block.streamPhase || "")
          .trim()
          .toLowerCase(),
        text,
        completed: Boolean(block.completed),
        transient: Boolean(block.transient),
        order: (order += 1),
      });
    }

    for (const activity of dedupeStreamingActivities(session.activities || [])) {
      const summary = String(activity?.summary || "").trim();
      if (!summary) continue;
      if (duplicatesTranscriptText(activity, narrativeTranscriptTexts)) continue;
      entries.push({
        id: `activity:${pendingEventId}:${String(activity.id || "").trim() || order}`,
        kind: "activity",
        pendingEventId,
        ts: String(activity.ts || session.updatedAt || "").trim(),
        live:
          liveSession &&
          String(activity.status || "")
            .trim()
            .toLowerCase() !== "completed",
        activity: {
          ...activity,
          id: String(activity.id || "").trim() || `activity-${order}`,
          summary,
        },
        order: (order += 1),
      });
    }
  }

  return entries.sort(compareTimelineEntries).map(({ order: _order, ...entry }) => entry);
}

export function buildHeadlessPreviewRenderGroups(
  entries: HeadlessPreviewTimelineEntry[],
): HeadlessPreviewRenderGroup[] {
  const groups: HeadlessPreviewRenderGroup[] = [];
  let activityBand: Array<Extract<HeadlessPreviewTimelineEntry, { kind: "activity" }>> = [];

  const flushActivityBand = () => {
    if (activityBand.length <= 0) return;
    const first = activityBand[0];
    const last = activityBand[activityBand.length - 1];
    groups.push({
      id: `activity-band:${first.id}:${last.id}`,
      kind: "activity-band",
      pendingEventId: first.pendingEventId,
      ts: String(last.ts || first.ts || "").trim(),
      live: activityBand.some((entry) => entry.live),
      entries: activityBand,
    });
    activityBand = [];
  };

  for (const entry of entries) {
    if (entry.kind === "activity") {
      if (activityBand.length > 0 && activityBand[0]?.pendingEventId !== entry.pendingEventId) {
        flushActivityBand();
      }
      activityBand.push(entry);
      continue;
    }

    flushActivityBand();
    groups.push({
      id: `message:${entry.id}`,
      kind: "message",
      pendingEventId: entry.pendingEventId,
      ts: entry.ts,
      live: entry.live,
      entry,
    });
  }

  flushActivityBand();
  return groups;
}
