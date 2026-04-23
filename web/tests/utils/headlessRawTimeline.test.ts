import { describe, expect, it } from "vitest";

import { buildHeadlessRawTraceEntries } from "../../src/utils/headlessRawTimeline";

describe("buildHeadlessRawTraceEntries", () => {
  it("moves a streaming message forward when later deltas arrive", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.message.started",
        ts: "2026-04-23T10:00:00Z",
        data: { stream_id: "stream-1", phase: "commentary" },
      },
      {
        actor_id: "coder",
        type: "headless.activity.updated",
        ts: "2026-04-23T10:00:01Z",
        data: { activity_id: "cmd-1", summary: "git status", kind: "command", raw_item_type: "commandExecution", status: "updated" },
      },
      {
        actor_id: "coder",
        type: "headless.message.delta",
        ts: "2026-04-23T10:00:02Z",
        data: { stream_id: "stream-1", phase: "commentary", delta: "Still working" },
      },
    ]);

    expect(entries.map((entry) => entry.kind)).toEqual(["event", "message"]);
    expect(entries[0]).toMatchObject({ kind: "event", badge: "RUN" });
    expect(entries[1]).toMatchObject({ kind: "message", ts: "2026-04-23T10:00:02Z", text: "Still working" });
  });

  it("keeps message text accumulated across deltas and completion", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.message.started",
        ts: "2026-04-23T10:00:00Z",
        data: { stream_id: "stream-1", phase: "final_answer" },
      },
      {
        actor_id: "coder",
        type: "headless.message.delta",
        ts: "2026-04-23T10:00:01Z",
        data: { stream_id: "stream-1", phase: "final_answer", delta: "Hello" },
      },
      {
        actor_id: "coder",
        type: "headless.message.completed",
        ts: "2026-04-23T10:00:02Z",
        data: { stream_id: "stream-1", phase: "final_answer", text: "Hello world" },
      },
    ]);

    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      kind: "message",
      streamPhase: "final_answer",
      text: "Hello world",
      completed: true,
      live: false,
      ts: "2026-04-23T10:00:02Z",
    });
  });

  it("extracts failed turn errors into a visible error event", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.turn.failed",
        ts: "2026-04-23T10:00:03Z",
        data: {
          turn_id: "turn-1",
          status: "error",
          error: { message: "tool crashed", detail: "exit code 1" },
        },
      },
    ]);

    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      kind: "event",
      badge: "TURN",
      tone: "error",
      title: "error",
    });
    expect(entries[0]?.kind === "event" ? entries[0].detailLines : []).toContain("tool crashed | exit code 1");
  });
});
