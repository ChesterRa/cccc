import { describe, expect, it } from "vite-plus/test";
import type { Actor, StreamingActivity } from "../../types";
import { buildLiveWorkCards } from "./liveWorkCards";

describe("live work cards", () => {
  it("projects PTY runtime activities without creating a chat placeholder", () => {
    const activity: StreamingActivity = {
      id: "tool:1",
      kind: "tool",
      status: "started",
      summary: "Calling Bash",
      ts: "2026-07-28T00:00:00Z",
    };
    const cards = buildLiveWorkCards({
      actors: [{ id: "peer", runner: "pty", runtime: "codex" } as Actor],
      events: [],
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: {},
      runtimeActivitiesByActorId: { peer: [activity] },
      replySessionsByPendingEventId: {},
    });
    expect(cards).toHaveLength(1);
    expect(cards[0]?.pendingEventId).toBe("");
    expect(cards[0]?.activities).toEqual([activity]);
    expect(cards[0]?.phase).toBe("streaming");
  });

  it.each([
    ["completed", "completed"],
    ["failed", "failed"],
  ] as const)("maps a terminal tool activity to %s instead of streaming", (status, phase) => {
    const activity: StreamingActivity = {
      id: "tool:1",
      kind: "tool",
      status,
      summary: `Bash ${status}`,
      ts: "2026-07-28T00:00:00Z",
      tool_name: "Bash",
    };
    const cards = buildLiveWorkCards({
      actors: [{ id: "peer", runner: "pty", runtime: "codex" } as Actor],
      events: [],
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: {},
      runtimeActivitiesByActorId: { peer: [activity] },
      replySessionsByPendingEventId: {},
    });

    expect(cards[0]?.phase).toBe(phase);
    expect(cards[0]?.updatedAt).toBe(activity.ts);
  });

  it("uses the latest terminal tool result instead of any historical failure", () => {
    const activities: StreamingActivity[] = [
      {
        id: "tool:failed",
        kind: "tool",
        status: "failed",
        summary: "Read failed",
        ts: "2026-07-28T00:00:00Z",
        tool_name: "Read",
      },
      {
        id: "tool:completed",
        kind: "tool",
        status: "completed",
        summary: "Bash completed",
        ts: "2026-07-28T00:00:01Z",
        tool_name: "Bash",
      },
    ];
    const cards = buildLiveWorkCards({
      actors: [{ id: "peer", runner: "pty", runtime: "codex" } as Actor],
      events: [],
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: {},
      runtimeActivitiesByActorId: { peer: activities },
      replySessionsByPendingEventId: {},
    });

    expect(cards[0]?.phase).toBe("completed");
  });
});
