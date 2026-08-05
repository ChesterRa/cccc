import type { StreamingReplySession } from "../../stores/chatStreamingSessions";
import type {
  Actor,
  ChatMessageData,
  HeadlessPreviewBlock,
  HeadlessPreviewSession,
  LedgerEvent,
  StreamingActivity,
} from "../../types";
import { isHeadlessActorRunner } from "../../utils/headlessRuntimeSupport";
import { resolveRuntimeActivityPhase } from "./runtimeActivityPhase";

export type LiveWorkPhase = "pending" | "streaming" | "completed" | "failed";

export type LiveWorkCard = {
  actorId: string;
  actorLabel: string;
  runtime: string;
  phase: LiveWorkPhase;
  streamPhase: string;
  text: string;
  transcriptBlocks: HeadlessPreviewBlock[];
  activities: StreamingActivity[];
  runtimeActivities?: StreamingActivity[];
  previewSessions?: HeadlessPreviewSession[];
  updatedAt: string;
  streamId: string;
  pendingEventId: string;
};

function isHeadlessActor(actor: Actor): boolean {
  return isHeadlessActorRunner(actor);
}

function normalizeActivities(value: unknown): StreamingActivity[] {
  if (!Array.isArray(value)) return [];
  return value.filter(Boolean) as StreamingActivity[];
}

function normalizeSessionTime(updatedAt: number | undefined): string {
  if (!Number.isFinite(Number(updatedAt))) return "";
  try {
    return new Date(Number(updatedAt)).toISOString();
  } catch {
    return "";
  }
}

function resolvePhase(args: {
  session: StreamingReplySession | undefined;
  event: LedgerEvent | undefined;
  pendingPlaceholder: boolean;
  hasRenderableContent: boolean;
}): LiveWorkPhase {
  const sessionPhase = String(args.session?.phase || "")
    .trim()
    .toLowerCase();
  if (
    sessionPhase === "pending" ||
    sessionPhase === "streaming" ||
    sessionPhase === "completed" ||
    sessionPhase === "failed"
  ) {
    return sessionPhase;
  }
  if (args.event?._streaming) {
    return args.pendingPlaceholder && !args.hasRenderableContent ? "pending" : "streaming";
  }
  if (args.pendingPlaceholder && !args.hasRenderableContent) {
    return "pending";
  }
  return "completed";
}

function phasePriority(phase: LiveWorkPhase): number {
  switch (phase) {
    case "streaming":
      return 0;
    case "pending":
      return 1;
    case "failed":
      return 2;
    case "completed":
    default:
      return 3;
  }
}

export function buildLiveWorkCards(args: {
  actors: Actor[];
  events: LedgerEvent[];
  latestActorPreviewByActorId: Record<string, HeadlessPreviewSession>;
  previewSessionsByActorId?: Record<string, HeadlessPreviewSession[]>;
  latestActorTextByActorId: Record<string, string>;
  latestActorActivitiesByActorId: Record<string, StreamingActivity[]>;
  runtimeActivitiesByActorId?: Record<string, StreamingActivity[]>;
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
}): LiveWorkCard[] {
  const latestEventByActorId = new Map<string, LedgerEvent>();
  for (const event of Array.isArray(args.events) ? args.events : []) {
    if (String(event.kind || "").trim() !== "chat.message") continue;
    if (typeof event._streaming !== "boolean") continue;
    const actorId = String(event.by || "").trim();
    if (!actorId || actorId === "user") continue;
    latestEventByActorId.set(actorId, event);
  }

  const latestSessionByActorId = new Map<string, StreamingReplySession>();
  for (const session of Object.values(args.replySessionsByPendingEventId || {})) {
    const actorId = String(session?.actorId || "").trim();
    if (!actorId) continue;
    const existing = latestSessionByActorId.get(actorId);
    if (!existing || Number(session.updatedAt || 0) >= Number(existing.updatedAt || 0)) {
      latestSessionByActorId.set(actorId, session);
    }
  }

  const cards: LiveWorkCard[] = [];
  for (const actor of Array.isArray(args.actors) ? args.actors : []) {
    const actorId = String(actor.id || "").trim();
    if (!actorId) continue;
    const runtimeActivities = Array.isArray(args.runtimeActivitiesByActorId?.[actorId])
      ? args.runtimeActivitiesByActorId?.[actorId] || []
      : [];
    const headless = isHeadlessActor(actor);
    const event = latestEventByActorId.get(actorId);
    const session = latestSessionByActorId.get(actorId);
    if ((!headless || (!event && !session)) && runtimeActivities.length <= 0) continue;
    const previewSessions = Array.isArray(args.previewSessionsByActorId?.[actorId])
      ? (args.previewSessionsByActorId?.[actorId] || []).filter(Boolean)
      : [];
    const preview =
      previewSessions.length > 0
        ? previewSessions[previewSessions.length - 1]
        : args.latestActorPreviewByActorId?.[actorId];

    const data =
      event?.data && typeof event.data === "object"
        ? (event.data as ChatMessageData & {
            pending_placeholder?: unknown;
            pending_event_id?: unknown;
          })
        : undefined;
    const transcriptBlocks = Array.isArray(preview?.transcriptBlocks)
      ? preview?.transcriptBlocks.filter((block) => String(block?.text || "").trim())
      : [];
    const text = String(
      preview?.latestText || args.latestActorTextByActorId?.[actorId] || data?.text || "",
    ).trim();
    const headlessActivities = Array.isArray(preview?.activities)
      ? preview.activities
      : Array.isArray(args.latestActorActivitiesByActorId?.[actorId])
        ? args.latestActorActivitiesByActorId[actorId]
        : normalizeActivities(data?.activities);
    const activities = Array.from(
      new Map(
        [...headlessActivities, ...runtimeActivities].map((activity) => [activity.id, activity]),
      ).values(),
    );
    const pendingPlaceholder = Boolean(data?.pending_placeholder);
    const basePhase = resolvePhase({
      session,
      event,
      pendingPlaceholder,
      hasRenderableContent: Boolean(text) || transcriptBlocks.length > 0 || activities.length > 0,
    });
    const phase = resolveRuntimeActivityPhase(basePhase, runtimeActivities);
    if (!text && transcriptBlocks.length === 0 && activities.length === 0 && phase === "completed")
      continue;

    cards.push({
      actorId,
      actorLabel: String(actor.title || actorId),
      runtime: String(actor.runtime || "headless").trim() || "headless",
      phase,
      streamPhase: String(preview?.streamPhase || data?.stream_phase || "")
        .trim()
        .toLowerCase(),
      text,
      transcriptBlocks,
      activities,
      runtimeActivities,
      previewSessions,
      updatedAt:
        String(preview?.updatedAt || event?.ts || "").trim() ||
        normalizeSessionTime(session?.updatedAt) ||
        String(runtimeActivities[runtimeActivities.length - 1]?.ts || "").trim(),
      streamId: String(
        preview?.currentStreamId || data?.stream_id || session?.currentStreamId || "",
      ).trim(),
      pendingEventId: String(
        preview?.pendingEventId || session?.pendingEventId || data?.pending_event_id || "",
      ).trim(),
    });
  }

  return cards.sort((left, right) => {
    const phaseDelta = phasePriority(left.phase) - phasePriority(right.phase);
    if (phaseDelta !== 0) return phaseDelta;
    const leftTs = String(left.updatedAt || "").trim();
    const rightTs = String(right.updatedAt || "").trim();
    if (leftTs && rightTs && leftTs !== rightTs) return rightTs.localeCompare(leftTs);
    if (leftTs && !rightTs) return -1;
    if (!leftTs && rightTs) return 1;
    return left.actorLabel.localeCompare(right.actorLabel);
  });
}
