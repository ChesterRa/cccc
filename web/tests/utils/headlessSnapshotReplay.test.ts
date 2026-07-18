import { describe, expect, it } from "vite-plus/test";

import type { HeadlessStreamEvent } from "../../src/types";
import { replayHeadlessSnapshotEvents } from "../../src/utils/headlessSnapshotReplay";

describe("replayHeadlessSnapshotEvents", () => {
  it("replays turn, activity, and message events in order", () => {
    const seen: string[] = [];
    const events: HeadlessStreamEvent[] = [
      {
        actor_id: "coder",
        type: "headless.turn.started",
        ts: "2026-04-08T10:00:00Z",
        data: { turn_id: "turn-1" },
      },
      {
        actor_id: "coder",
        type: "headless.activity.started",
        ts: "2026-04-08T10:00:01Z",
        data: { activity_id: "act-1", summary: "Inspect" },
      },
      {
        actor_id: "coder",
        type: "headless.message.delta",
        ts: "2026-04-08T10:00:02Z",
        data: { stream_id: "stream-1", delta: "Hel" },
      },
      {
        actor_id: "coder",
        type: "headless.message.completed",
        ts: "2026-04-08T10:00:03Z",
        data: { stream_id: "stream-1", text: "Hello" },
      },
    ];

    replayHeadlessSnapshotEvents(events, (event) => {
      seen.push(String(event.type || ""));
    });

    expect(seen).toEqual([
      "headless.turn.started",
      "headless.activity.started",
      "headless.message.delta",
      "headless.message.completed",
    ]);
  });

  it("skips malformed entries without dropping valid replay events", () => {
    const seen: string[] = [];
    const events = [
      { actor_id: "", type: "headless.turn.started", ts: "2026-04-08T10:00:00Z", data: {} },
      { actor_id: "coder", type: "", ts: "2026-04-08T10:00:01Z", data: {} },
      {
        actor_id: "coder",
        type: "headless.activity.updated",
        ts: "2026-04-08T10:00:02Z",
        data: { activity_id: "act-1", summary: "Inspect more" },
      },
    ] as HeadlessStreamEvent[];

    replayHeadlessSnapshotEvents(events, (event) => {
      seen.push(String(event.type || ""));
    });

    expect(seen).toEqual(["headless.activity.updated"]);
  });
});
