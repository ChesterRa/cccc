import type { HeadlessStreamEvent } from "../../types";
import type { RuntimeDockItem } from "./runtimeDockItems";

export type RuntimeDockTickerEntry = {
  id: string;
  kind: "message" | "activity";
  actorId: string;
  actorLabel: string;
  text: string;
  updatedAt: string;
  receivedAt: number;
  sourceId?: string;
  completed?: boolean;
};

/** Preview history is for inspection. Only observed live events may become bubbles. */
export function buildRuntimeDockTickerEntries(
  items: RuntimeDockItem[],
  eventsByActor: Record<string, HeadlessStreamEvent[]> = {},
): RuntimeDockTickerEntry[] {
  const entries: RuntimeDockTickerEntry[] = [];
  for (const item of items) {
    const events = eventsByActor[item.actorId] || [];
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index];
      if (!event?._receivedAt) continue;
      const data = event.data || {};
      const type = String(event.type || "");
      const streamId = String(data.stream_id || "");
      const phase = String(data.phase || "");
      const isMessage = type === "headless.message.delta" || type === "headless.message.completed";
      const isActivity = type.startsWith("headless.activity.");
      if (!isMessage && !isActivity) continue;
      let text = String(isMessage ? data.text || "" : data.summary || "").trim();
      if (isMessage && !text) {
        const card = item.liveWorkCard;
        const blocks = [
          ...(card?.previewSessions || []).flatMap((session) => session.transcriptBlocks),
          ...(card?.transcriptBlocks || []),
        ];
        const block = [...blocks]
          .reverse()
          .find(
            (candidate) =>
              candidate.streamId === streamId &&
              candidate.streamPhase === phase &&
              candidate.updatedAt === event.ts,
          );
        // Delta receipt precedes the batched preview update. Wait for its projection.
        if (!block) break;
        text = block.text.trim();
      }
      if (!text) continue;
      const sourceId = [
        item.actorId,
        isMessage ? "message" : "activity",
        streamId || data.event_id || "",
        isMessage ? phase : data.activity_id || data.id || event.id,
      ].join(":");
      entries.push({
        id: item.actorId,
        kind: isMessage ? "message" : "activity",
        actorId: item.actorId,
        actorLabel: item.actorLabel,
        text,
        sourceId,
        updatedAt: String(event.ts || ""),
        receivedAt: event._receivedAt,
        completed: type.endsWith(".completed"),
      });
      break;
    }
  }
  return entries.sort((a, b) => a.receivedAt - b.receivedAt);
}
