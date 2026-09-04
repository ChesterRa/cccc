import { describe, expect, it } from "vite-plus/test";
import type { Actor, HeadlessPreviewSession, StreamingActivity } from "../../types";
import { buildLiveWorkCards } from "./liveWorkCards";

function preview(
  phase: HeadlessPreviewSession["phase"],
  activities: StreamingActivity[],
): HeadlessPreviewSession {
  return {
    actorId: "peer",
    pendingEventId: "turn-1",
    currentStreamId: "stream-1",
    phase,
    streamPhase: phase,
    updatedAt: "2026-07-28T00:00:00Z",
    latestText: "",
    transcriptBlocks: [],
    activities,
  };
}

function cards(session: HeadlessPreviewSession) {
  return buildLiveWorkCards({
    actors: [
      {
        id: "peer",
        runner: "pty",
        runtime: "codex",
        runtime_state_source: "managed_session",
      } as Actor,
    ],
    events: [],
    latestActorPreviewByActorId: { peer: session },
    latestActorTextByActorId: {},
    latestActorActivitiesByActorId: {},
    replySessionsByPendingEventId: {},
  });
}

describe("live work cards", () => {
  it("projects managed-session activity without a ledger placeholder", () => {
    const activity: StreamingActivity = {
      id: "tool:1",
      kind: "tool",
      status: "started",
      summary: "Calling Bash",
      ts: "2026-07-28T00:00:00Z",
    };

    const result = cards(preview("streaming", [activity]));

    expect(result).toHaveLength(1);
    expect(result[0]?.pendingEventId).toBe("turn-1");
    expect(result[0]?.activities).toEqual([activity]);
    expect(result[0]?.phase).toBe("streaming");
  });

  it.each(["completed", "failed"] as const)(
    "uses the managed event stream's %s terminal phase",
    (phase) => {
      const activity: StreamingActivity = {
        id: "tool:1",
        kind: "tool",
        status: phase,
        summary: `Bash ${phase}`,
        ts: "2026-07-28T00:00:00Z",
        tool_name: "Bash",
      };

      const result = cards(preview(phase, [activity]));

      expect(result[0]?.phase).toBe(phase);
      expect(result[0]?.updatedAt).toBe("2026-07-28T00:00:00Z");
    },
  );

  it("does not project raw PTY actors as structured activity", () => {
    const session = preview("streaming", []);
    const result = buildLiveWorkCards({
      actors: [{ id: "peer", runner: "pty", runtime: "custom" } as Actor],
      events: [],
      latestActorPreviewByActorId: { peer: session },
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: {},
      replySessionsByPendingEventId: {},
    });

    expect(result).toEqual([]);
  });
});
