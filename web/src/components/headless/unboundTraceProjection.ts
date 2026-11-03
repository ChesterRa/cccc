import type { HeadlessPreviewSession, LedgerEvent, StreamingActivity } from "../../types";

/** Present the latest unbound headless text alongside any pending previews,
 * without inventing a pending event or adding a separate retained session. */
export function projectUnboundTrace(
  groupId: string,
  actorId: string,
  streamingEvents: LedgerEvent[],
  boundSessions: HeadlessPreviewSession[],
): HeadlessPreviewSession | null {
  const boundStreams = new Set<string>();
  for (const session of boundSessions) {
    if (session.currentStreamId) boundStreams.add(session.currentStreamId);
    for (const block of session.transcriptBlocks) {
      if (block.streamId) boundStreams.add(block.streamId);
    }
  }
  let latest: { row: LedgerEvent; data: Record<string, unknown>; streamId: string } | null = null;
  for (const row of streamingEvents) {
    if ((row.group_id && row.group_id !== groupId) || row.by !== actorId) continue;
    if (!row.data || typeof row.data !== "object") continue;
    const data = row.data as Record<string, unknown>;
    const streamId = String(data.stream_id || "").trim();
    if (!streamId || data.pending_event_id || data.reply_to || boundStreams.has(streamId)) continue;
    if (!data._headless_text_started_at || !String(data.text || "").trim()) continue;
    if (!latest || String(row.ts || "") >= String(latest.row.ts || ""))
      latest = { row, data, streamId };
  }
  if (!latest) return null;
  const { row, data, streamId } = latest;
  const text = String(data.text || "").trim();
  const streamPhase = String(data.stream_phase || "");
  const messageCompleted = !row._streaming;
  return {
    actorId,
    pendingEventId: "",
    currentStreamId: streamId,
    phase: messageCompleted ? "completed" : "streaming",
    streamPhase,
    updatedAt: String(row.ts || ""),
    latestText: text,
    activities: Array.isArray(data.activities) ? (data.activities as StreamingActivity[]) : [],
    transcriptBlocks: [
      {
        id: `stream:${streamId}`,
        streamId,
        streamPhase,
        text,
        updatedAt: String(data._headless_text_started_at || ""),
        completed: messageCompleted,
        transient: Boolean(data.transient_stream),
      },
    ],
  };
}
