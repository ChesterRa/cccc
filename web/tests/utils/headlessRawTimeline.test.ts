import { describe, expect, it } from "vite-plus/test";

import {
  buildHeadlessRawTraceEntries,
  buildHeadlessRawTraceRenderGroups,
} from "../../src/utils/headlessRawTimeline";

describe("buildHeadlessRawTraceEntries", () => {
  it.each([
    ["headless.turn.started", "info", true],
    ["headless.turn.completed", "success", false],
    ["headless.turn.failed", "error", false],
    ["headless.turn.cancelled", "neutral", false],
    ["headless.control.started", "info", true],
    ["headless.control.queued", "warning", false],
    ["headless.control.requeued", "warning", false],
    ["headless.control.stalled", "warning", false],
    ["headless.control.completed", "success", false],
    ["headless.control.failed", "error", false],
    ["headless.control.unknown_future_state", "neutral", false],
  ] as const)(
    "classifies %s without inventing a running or successful state",
    (type, tone, live) => {
      const entries = buildHeadlessRawTraceEntries([
        { actor_id: "coder", type, ts: "2026-04-23T10:00:00Z", data: { turn_id: "turn-1" } },
      ]);
      expect(entries).toHaveLength(1);
      expect(entries[0]).toMatchObject({ kind: "event", tone, live });
    },
  );

  it("settles a started turn as cancelled, not as running", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.turn.started",
        ts: "2026-04-23T10:00:00Z",
        data: { turn_id: "turn-1" },
      },
      {
        actor_id: "coder",
        type: "headless.turn.cancelled",
        ts: "2026-04-23T10:00:01Z",
        data: { turn_id: "turn-1", status: "cancelled" },
      },
    ]);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ title: "cancelled", tone: "neutral", live: false });
    expect(buildHeadlessRawTraceRenderGroups(entries)[0]).toMatchObject({ live: false });
  });

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
        data: {
          activity_id: "cmd-1",
          summary: "git status",
          kind: "command",
          raw_item_type: "commandExecution",
          status: "updated",
        },
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
    expect(entries[1]).toMatchObject({
      kind: "message",
      ts: "2026-04-23T10:00:02Z",
      text: "Still working",
    });
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
    expect(entries[0]?.kind === "event" ? entries[0].detailLines : []).toContain(
      "tool crashed | exit code 1",
    );
  });

  it("groups consecutive non-error trace events into one reasoning band", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.activity.updated",
        ts: "2026-04-23T10:00:00Z",
        data: {
          activity_id: "cmd-1",
          summary: "rg -n foo",
          kind: "tool",
          raw_item_type: "commandExecution",
          status: "updated",
        },
      },
      {
        actor_id: "coder",
        type: "headless.control.queued",
        ts: "2026-04-23T10:00:01Z",
        data: { control_kind: "system_notify", status: "queued" },
      },
      {
        actor_id: "coder",
        type: "headless.message.delta",
        ts: "2026-04-23T10:00:02Z",
        data: { stream_id: "stream-1", phase: "commentary", delta: "Still working" },
      },
    ]);

    const groups = buildHeadlessRawTraceRenderGroups(entries);
    expect(groups).toHaveLength(2);
    expect(groups[0]).toMatchObject({ kind: "event-band", live: true });
    expect(
      groups[0]?.kind === "event-band" ? groups[0].entries.map((entry) => entry.badge) : [],
    ).toEqual(["RUN", "CONTROL"]);
    expect(groups[1]).toMatchObject({ kind: "message", live: true });
  });

  it("keeps error events outside the reasoning band", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.activity.updated",
        ts: "2026-04-23T10:00:00Z",
        data: {
          activity_id: "cmd-1",
          summary: "rg -n foo",
          kind: "tool",
          raw_item_type: "commandExecution",
          status: "updated",
        },
      },
      {
        actor_id: "coder",
        type: "headless.turn.failed",
        ts: "2026-04-23T10:00:01Z",
        data: { turn_id: "turn-1", status: "error", error: { message: "boom" } },
      },
    ]);

    const groups = buildHeadlessRawTraceRenderGroups(entries);
    expect(groups).toHaveLength(2);
    expect(groups[0]).toMatchObject({ kind: "event" });
    expect(groups[1]).toMatchObject({ kind: "event" });
    expect(groups[1]?.kind === "event" ? groups[1].entry.tone : "").toBe("error");
  });

  it("renders Codex resume failure as a visible state event", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.thread.resume_failed",
        ts: "2026-04-23T10:00:00Z",
        data: { thread_id: "thr-stale", status: "failed", error: "thread not found" },
      },
    ]);

    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      kind: "event",
      badge: "STATE",
      tone: "warning",
      title: "Resume failed",
    });
    expect(entries[0]?.kind === "event" ? entries[0].detailLines : []).toEqual([
      "thread: thr-stale",
      "thread not found",
    ]);
  });

  it("renders Claude session resume failure as a visible state event", () => {
    const entries = buildHeadlessRawTraceEntries([
      {
        actor_id: "coder",
        type: "headless.session.resume_failed",
        ts: "2026-04-23T10:00:00Z",
        data: { error: "No conversation found" },
      },
    ]);

    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      kind: "event",
      badge: "STATE",
      tone: "warning",
      title: "Resume failed",
    });
    expect(entries[0]?.kind === "event" ? entries[0].detailLines : []).toEqual([
      "No conversation found",
    ]);
  });
});
