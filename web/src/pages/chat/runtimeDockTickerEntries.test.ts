import { describe, expect, it } from "vite-plus/test";
import type { HeadlessPreviewSession, StreamingActivity } from "../../types";
import type { LiveWorkCard } from "./liveWorkCards";
import { buildRuntimeDockTickerEntries } from "./runtimeDockTickerEntries";
import type { RuntimeDockItem } from "./runtimeDockItems";

describe("runtime dock ticker entries", () => {
  it("keeps PTY activities when an actor already has preview sessions", () => {
    const runtimeActivity: StreamingActivity = {
      id: "tool:runtime",
      kind: "tool",
      status: "started",
      summary: "Calling Bash",
      ts: "2026-07-28T00:00:01Z",
    };
    const previewSession = {
      actorId: "foreman",
      pendingEventId: "message:1",
      currentStreamId: "stream:1",
      phase: "streaming",
      streamPhase: "streaming",
      updatedAt: "2026-07-28T00:00:00Z",
      latestText: "Existing preview",
      transcriptBlocks: [],
      activities: [],
    } as HeadlessPreviewSession;
    const card = {
      actorId: "foreman",
      actorLabel: "Foreman",
      runtime: "codex",
      phase: "streaming",
      streamPhase: "streaming",
      text: "",
      transcriptBlocks: [],
      activities: [runtimeActivity],
      runtimeActivities: [runtimeActivity],
      previewSessions: [previewSession],
      updatedAt: runtimeActivity.ts || "",
      streamId: "",
      pendingEventId: "",
    } satisfies LiveWorkCard;
    const item = {
      actorId: "foreman",
      actorLabel: "Foreman",
      liveWorkCard: card,
    } as RuntimeDockItem;

    expect(buildRuntimeDockTickerEntries([item])).toEqual([
      expect.objectContaining({ kind: "activity", actorId: "foreman", text: "Calling Bash" }),
    ]);
  });

  it("coalesces equivalent tool states from parallel operations", () => {
    const runtimeActivities: StreamingActivity[] = [
      {
        id: "tool:1",
        kind: "tool",
        status: "completed",
        summary: "Bash completed in 1s",
        tool_name: "Bash",
        ts: "2026-07-28T00:00:01Z",
      },
      {
        id: "tool:2",
        kind: "tool",
        status: "completed",
        summary: "Bash completed in 2s",
        tool_name: "Bash",
        ts: "2026-07-28T00:00:02Z",
      },
    ];
    const card = {
      actorId: "foreman",
      actorLabel: "Foreman",
      runtime: "codex",
      phase: "streaming",
      streamPhase: "streaming",
      text: "",
      transcriptBlocks: [],
      activities: runtimeActivities,
      runtimeActivities,
      previewSessions: [],
      updatedAt: "2026-07-28T00:00:02Z",
      streamId: "",
      pendingEventId: "",
    } satisfies LiveWorkCard;
    const item = {
      actorId: "foreman",
      actorLabel: "Foreman",
      liveWorkCard: card,
    } as RuntimeDockItem;

    expect(buildRuntimeDockTickerEntries([item])).toEqual([
      expect.objectContaining({
        kind: "activity",
        actorId: "foreman",
        text: "Bash completed in 2s",
      }),
    ]);
  });

  it("keeps a terminal runtime tool bubble without marking the card active", () => {
    const runtimeActivity: StreamingActivity = {
      id: "tool:runtime",
      kind: "tool",
      status: "completed",
      summary: "Bash completed in 2s",
      tool_name: "Bash",
      ts: "2026-07-28T00:00:02Z",
    };
    const card = {
      actorId: "foreman",
      actorLabel: "Foreman",
      runtime: "codex",
      phase: "completed",
      streamPhase: "",
      text: "",
      transcriptBlocks: [],
      activities: [runtimeActivity],
      runtimeActivities: [runtimeActivity],
      previewSessions: [],
      updatedAt: runtimeActivity.ts || "",
      streamId: "",
      pendingEventId: "",
    } satisfies LiveWorkCard;

    expect(
      buildRuntimeDockTickerEntries([
        { actorId: "foreman", actorLabel: "Foreman", liveWorkCard: card } as RuntimeDockItem,
      ]),
    ).toEqual([expect.objectContaining({ kind: "activity", text: "Bash completed in 2s" })]);
  });
});
