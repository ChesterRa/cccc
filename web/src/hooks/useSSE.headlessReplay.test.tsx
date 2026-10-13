// @vitest-environment happy-dom
import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useSSE } from "./useSSE";
import { useGroupStore, useUIStore } from "../stores";
import { buildLiveWorkCards } from "../pages/chat/liveWorkCards";
import { buildRuntimeDockTickerEntries } from "../pages/chat/runtimeDockTickerEntries";
import type { HeadlessStreamEvent, Actor } from "../types";

function stubEventSources() {
  const sources: FixtureSource[] = [];
  class FixtureSource {
    listeners = new Map<string, EventListener[]>();
    onopen: (() => void) | null = null;
    onerror: (() => void) | null = null;
    constructor(public url: string) {
      sources.push(this);
    }
    addEventListener(type: string, listener: EventListener) {
      this.listeners.set(type, [...(this.listeners.get(type) || []), listener]);
    }
    close() {}
    emit(type: string, value: unknown) {
      for (const listener of this.listeners.get(type) || [])
        listener(new MessageEvent(type, { data: JSON.stringify(value) }));
    }
  }
  vi.stubGlobal("EventSource", FixtureSource);
  return sources;
}

it("restores snapshots silently, ignores replayed deltas, and admits only fresh progress", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  const sources = stubEventSources();
  const actor: Actor = {
    id: "a",
    runtime: "claude",
    runner: "pty",
    runtime_state_source: "managed_session",
    running: true,
  };
  useGroupStore.setState({
    selectedGroupId: "g",
    actors: [actor],
    chatByGroup: {},
    refreshActors: vi.fn().mockResolvedValue(undefined),
  });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: true },
      actorsRef: { current: [actor] },
    });
    return null;
  }
  const host = document.createElement("div"),
    root = createRoot(host);
  const frames: HeadlessStreamEvent[] = [
    {
      id: "start",
      group_id: "g",
      actor_id: "a",
      type: "headless.control.started",
      ts: "2026-09-04T00:00:00Z",
      data: { turn_id: "t" },
    },
    {
      id: "delta",
      group_id: "g",
      actor_id: "a",
      type: "headless.message.delta",
      ts: "2026-09-04T00:00:01Z",
      data: { turn_id: "t", stream_id: "s", delta: "Old complete response." },
    },
    {
      id: "done",
      group_id: "g",
      actor_id: "a",
      type: "headless.message.completed",
      ts: "2026-09-04T00:00:02Z",
      data: { turn_id: "t", stream_id: "s", text: "Old complete response." },
    },
    {
      id: "end",
      group_id: "g",
      actor_id: "a",
      type: "headless.control.completed",
      ts: "2026-09-04T00:00:03Z",
      data: { turn_id: "t", status: "completed" },
    },
  ];
  const project = () => {
    const b = useGroupStore.getState().chatByGroup.g!;
    const cards = buildLiveWorkCards({
      actors: [actor],
      events: b.streamingEvents,
      latestActorPreviewByActorId: b.latestActorPreviewByActorId,
      previewSessionsByActorId: b.previewSessionsByActorId,
      latestActorTextByActorId: b.latestActorTextByActorId,
      latestActorActivitiesByActorId: b.latestActorActivitiesByActorId,
      replySessionsByPendingEventId: b.replySessionsByPendingEventId,
    });
    return {
      text: b.streamingTextByStreamId.s,
      entries: buildRuntimeDockTickerEntries(
        cards.map((card) => ({
          actor,
          actorId: "a",
          actorLabel: "a",
          runtime: "claude",
          runner: "pty",
          unreadCount: 0,
          webModelQueuedCount: 0,
          liveWorkCard: card,
        })),
        b.rawHeadlessEventsByActorId,
      ),
    };
  };
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => connection!.connectStream("g"));
    const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
    await act(async () => stream.emit("headless.snapshot", { events: frames }));
    expect(project()).toEqual({ text: "Old complete response.", entries: [] });
    for (const frame of frames) {
      await act(async () => {
        stream.emit("headless", frame);
        await new Promise((resolve) => setTimeout(resolve, 25));
      });
      expect(project()).toEqual({ text: "Old complete response.", entries: [] });
    }
    await act(async () => stream.emit("headless.snapshot", { events: frames }));
    expect(project().entries).toEqual([]);
    const next = {
      ...frames[2],
      id: "new",
      ts: "2026-09-06T00:00:00Z",
      data: { turn_id: "t2", stream_id: "s2", text: "Fresh result." },
    };
    await act(async () => {
      stream.emit("headless", next);
      await new Promise((resolve) => setTimeout(resolve, 25));
    });
    expect(project().entries.map((entry) => entry.text)).toEqual(["Fresh result."]);
    const receivedAt = project().entries[0]!.receivedAt;
    await act(async () => {
      stream.emit("headless", next);
      await new Promise((resolve) => setTimeout(resolve, 25));
    });
    expect(project().entries[0]!.receivedAt).toBe(receivedAt);
    await act(async () =>
      stream.emit("headless", { ...next, id: "wrong-group", group_id: "other" }),
    );
    expect(project().entries[0]!.receivedAt).toBe(receivedAt);
  } finally {
    await act(async () => {
      connection!.cleanup();
      root.unmount();
    });
    vi.unstubAllGlobals();
    useGroupStore.setState(original);
    host.remove();
  }
});

it.each(["group switch", "unmount"])(
  "preserves buffered text and activities across %s before the animation frame",
  async (transition) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const original = useGroupStore.getState();
    const originalUI = useUIStore.getState();
    const sources = stubEventSources();
    const frames = new Map<number, FrameRequestCallback>();
    let frameId = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.set(++frameId, callback);
      return frameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
      frames.delete(id);
    });
    const flushFrame = () => {
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(performance.now());
    };
    const actor: Actor = {
      id: "a",
      runtime: "claude",
      runner: "pty",
      runtime_state_source: "managed_session",
      running: true,
    };
    useGroupStore.setState({ selectedGroupId: "g", actors: [actor], chatByGroup: {} });
    function Probe() {
      const groupId = useGroupStore((state) => state.selectedGroupId);
      const { connectStream, cleanup } = useSSE({
        activeTabRef: { current: "chat" },
        chatAtBottomRef: { current: true },
        actorsRef: { current: [actor] },
      });
      useEffect(() => {
        connectStream(groupId);
        return cleanup;
        // eslint-disable-next-line react-hooks/exhaustive-deps -- mirror the App's group lifecycle.
      }, [groupId]);
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    const delta: HeadlessStreamEvent = {
      id: "delta",
      group_id: "g",
      actor_id: "a",
      type: "headless.message.delta",
      ts: "2026-09-07T00:00:00Z",
      data: { stream_id: "s", event_id: "request", delta: "First " },
    };
    const activity: HeadlessStreamEvent = {
      ...delta,
      id: "activity",
      type: "headless.activity.started",
      data: {
        stream_id: "s",
        event_id: "request",
        activity_id: "tool",
        kind: "tool",
        summary: "Checking results",
      },
    };
    const bucket = () => useGroupStore.getState().chatByGroup.g!;
    const expectProjection = (text: string, status = "started") => {
      expect(bucket().streamingTextByStreamId.s).toBe(text);
      expect(bucket().streamingActivitiesByStreamId.s).toEqual([
        expect.objectContaining({ id: "tool", summary: "Checking results", status }),
      ]);
    };
    const latestStream = () =>
      sources.filter((source) => source.url.includes("/groups/g/headless/stream")).at(-1)!;
    try {
      await act(async () => root.render(<Probe />));
      const oldStream = latestStream();
      await act(async () => {
        oldStream.emit("headless", delta);
        oldStream.emit("headless", activity);
      });
      expect(bucket().rawHeadlessEventsByActorId.a.map((event) => event.id)).toEqual([
        "delta",
        "activity",
      ]);
      expect(bucket().streamingTextByStreamId.s).toBeUndefined();
      expect(bucket().streamingActivitiesByStreamId.s).toBeUndefined();
      expect(frames.size).toBe(2);

      await act(async () => {
        if (transition === "group switch") useGroupStore.setState({ selectedGroupId: "other" });
        else root.render(null);
      });
      expectProjection("First ");
      expect(frames.size).toBe(0);
      expect(useGroupStore.getState().chatByGroup.other).toBeUndefined();
      const retained = bucket();
      await act(async () => {
        oldStream.emit("headless", {
          ...delta,
          id: "stale",
          data: { ...delta.data, delta: "BAD" },
        });
        flushFrame();
      });
      expect(bucket()).toBe(retained);

      await act(async () => {
        if (transition === "group switch") useGroupStore.setState({ selectedGroupId: "g" });
        else root.render(<Probe />);
      });
      const stream = latestStream();
      await act(async () => stream.emit("headless.snapshot", { events: [delta, activity] }));
      expectProjection("First ");
      expect(bucket().rawHeadlessEventsByActorId.a).toHaveLength(2);
      await act(async () => {
        stream.emit("headless", {
          ...delta,
          id: "suffix",
          data: { ...delta.data, delta: "second." },
        });
        stream.emit("headless", {
          ...activity,
          id: "activity-done",
          type: "headless.activity.completed",
        });
        flushFrame();
      });
      expectProjection("First second.", "completed");
      expect(useGroupStore.getState().chatByGroup.other).toBeUndefined();
    } finally {
      await act(async () => root.unmount());
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      useUIStore.setState(originalUI);
      host.remove();
    }
  },
);

it("reconciles scope changes with fresh Group documents and rejects late refreshes", async () => {
  const api = await import("../services/api");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  const originalUI = useUIStore.getState();
  const sources = stubEventSources();
  const group = { group_id: "g", active_scope_key: "a", scopes: [{ scope_key: "a", url: "/a" }] };
  useGroupStore.setState({ selectedGroupId: "g", groupDoc: group, actors: [], chatByGroup: {} });
  const finish: Array<(value: unknown) => void> = [];
  const fetch = vi.spyOn(api, "fetchGroup").mockImplementation(
    () =>
      new Promise((resolve) => {
        finish.push(resolve as (value: unknown) => void);
      }),
  );
  const host = document.createElement("div"),
    root = createRoot(host);
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: true },
      actorsRef: { current: [] },
    });
    useEffect(() => {
      connection.connectStream("g");
      return () => connection.cleanup();
    }, []);
    return null;
  }
  try {
    await act(async () => root.render(<Probe />));
    const ledger = sources.find((source) => source.url.includes("/ledger/stream"))!;
    await act(async () =>
      ledger.emit("ledger", {
        id: "change-1",
        group_id: "g",
        kind: "group.set_active_scope",
        scope_key: "historical",
        data: {},
      }),
    );
    await act(async () =>
      ledger.emit("ledger", {
        id: "change-2",
        group_id: "g",
        kind: "group.attach",
        scope_key: "historical",
        data: {},
      }),
    );
    expect(fetch).toHaveBeenCalledWith("g", { noCache: true });
    expect(finish).toHaveLength(2);
    await act(async () =>
      finish[1]({ ok: true, result: { group: { ...group, active_scope_key: "b" } } }),
    );
    expect(useGroupStore.getState().groupDoc?.active_scope_key).toBe("b");
    await act(async () => finish[0]({ ok: true, result: { group } }));
    expect(useGroupStore.getState().groupDoc?.active_scope_key).toBe("b");
    await act(async () =>
      ledger.emit("ledger", {
        id: "change-3",
        group_id: "g",
        kind: "group.detach_scope",
        data: {},
      }),
    );
    await act(async () =>
      useGroupStore.setState({ selectedGroupId: "other", groupDoc: { group_id: "other" } }),
    );
    await act(async () => finish[2]({ ok: true, result: { group } }));
    expect(useGroupStore.getState().groupDoc?.group_id).toBe("other");
  } finally {
    await act(async () => root.unmount());
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    useGroupStore.setState(original);
    useUIStore.setState(originalUI);
    host.remove();
  }
});
// These tests exercise event consumers; transport multiplexing has its own wire tests.
vi.mock("../services/realtime/eventStream", () => ({
  openEventStream: (url: string) => new EventSource(url),
}));

it("shows an ACP provider failure without a pending chat event and deduplicates replay", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  const sources = stubEventSources();
  const actor: Actor = {
    id: "acp",
    runtime: "antigravity",
    runtime_mode: "acp",
    runner: "headless",
    runtime_state_source: "managed_session",
    running: true,
  };
  useGroupStore.setState({
    selectedGroupId: "g",
    actors: [actor],
    chatByGroup: {},
    refreshActors: vi.fn().mockResolvedValue(undefined),
  });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: true },
      actorsRef: { current: [actor] },
    });
    return null;
  }
  const host = document.createElement("div"),
    root = createRoot(host);
  const failure: HeadlessStreamEvent = {
    id: "failure",
    group_id: "g",
    actor_id: "acp",
    type: "headless.turn.failed",
    ts: "2026-10-03T00:00:00Z",
    data: {
      turn_id: "acp-turn",
      stream_id: "generation:acp-turn",
      status: "failed",
      error: "Synthetic provider failure",
    },
  };
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => connection!.connectStream("g"));
    const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
    const started: HeadlessStreamEvent = {
      ...failure,
      id: "started",
      type: "headless.turn.started",
      data: { turn_id: "acp-turn", stream_id: "generation:acp-turn" },
    };
    const delta: HeadlessStreamEvent = {
      ...started,
      id: "delta",
      type: "headless.message.delta",
      data: { ...started.data, delta: "Partial answer" },
    };
    await act(async () => {
      stream.emit("headless", started);
      stream.emit("headless", delta);
    });
    expect(useGroupStore.getState().actors[0]!.effective_working_state).toBe("working");
    await act(async () => stream.emit("headless", failure));
    const bucket = () => useGroupStore.getState().chatByGroup.g!;
    expect(bucket().streamingActivitiesByStreamId["generation:acp-turn"]).toEqual([
      expect.objectContaining({
        kind: "error",
        detail: "Synthetic provider failure",
        raw_item_type: "turn_error",
      }),
    ]);
    expect(bucket().streamingEvents).toHaveLength(1);
    expect(bucket().streamingEvents[0]!._streaming).toBe(false);
    // The live projection agrees with the daemon Actor GET without claiming
    // that a rejected turn stopped the still-owned provider process.
    expect(useGroupStore.getState().actors[0]).toMatchObject({
      running: true,
      effective_working_state: "error",
      effective_working_reason: "Synthetic provider failure",
      effective_active_task_id: null,
    });
    await act(async () => stream.emit("headless", failure));
    expect(bucket().streamingEvents).toHaveLength(1);
    expect(bucket().streamingActivitiesByStreamId["generation:acp-turn"]).toHaveLength(1);
    await act(async () => stream.emit("headless.snapshot", { events: [started, delta, failure] }));
    expect(bucket().streamingActivitiesByStreamId["generation:acp-turn"]).toHaveLength(1);
    const nextStarted: HeadlessStreamEvent = {
      ...started,
      id: "next-started",
      data: { turn_id: "next-turn", stream_id: "generation:next-turn" },
    };
    await act(async () => stream.emit("headless", nextStarted));
    const recovered = useGroupStore.getState().actors[0]!;
    expect(recovered).toMatchObject({
      running: true,
      effective_working_state: "working",
      effective_working_reason: "headless_turn_active",
      effective_active_task_id: "next-turn",
    });
    await act(async () => stream.emit("headless", failure));
    expect(useGroupStore.getState().actors[0]).toBe(recovered);
    await act(async () =>
      stream.emit("headless", {
        ...nextStarted,
        id: "next-completed",
        type: "headless.turn.completed",
        data: { ...nextStarted.data, status: "completed" },
      }),
    );
    expect(useGroupStore.getState().actors[0]).toMatchObject({
      running: true,
      effective_working_state: "idle",
      effective_working_reason: "headless_turn_idle",
      effective_active_task_id: null,
    });
  } finally {
    await act(async () => {
      connection!.cleanup();
      root.unmount();
    });
    vi.unstubAllGlobals();
    useGroupStore.setState(original);
    host.remove();
  }
});

// A cancelled turn is a deliberate stop, not a failure. It must close the turn
// and clear the transient stream exactly like a completed one, and must never
// render as an error activity or set the failed working reason.
it.each(["cancelled", "completed"] as const)(
  "closes a %s active turn as idle without a failure",
  async (outcome) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const original = useGroupStore.getState();
    const sources = stubEventSources();
    const actor: Actor = {
      id: "acp",
      runtime: "antigravity",
      runtime_mode: "acp",
      runner: "headless",
      runtime_state_source: "managed_session",
      running: true,
    };
    useGroupStore.setState({
      selectedGroupId: "g",
      actors: [actor],
      chatByGroup: {},
      refreshActors: vi.fn().mockResolvedValue(undefined),
    });
    let connection: ReturnType<typeof useSSE>;
    function Probe() {
      connection = useSSE({
        activeTabRef: { current: "chat" },
        chatAtBottomRef: { current: true },
        actorsRef: { current: [actor] },
      });
      return null;
    }
    const host = document.createElement("div"),
      root = createRoot(host);
    const cancelled: HeadlessStreamEvent = {
      id: "cancelled",
      group_id: "g",
      actor_id: "acp",
      type: `headless.turn.${outcome}`,
      ts: "2026-10-03T00:00:00Z",
      data: { turn_id: "acp-turn", stream_id: "generation:acp-turn", status: outcome },
    };
    try {
      await act(async () => root.render(<Probe />));
      await act(async () => connection!.connectStream("g"));
      const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
      await act(async () => {
        stream.emit("headless", { ...cancelled, id: "start", type: "headless.turn.started" });
        stream.emit("headless", {
          ...cancelled,
          id: "partial",
          type: "headless.message.delta",
          data: { ...cancelled.data, phase: "commentary", delta: "Temporary progress" },
        });
      });
      expect(useGroupStore.getState().actors[0]!.effective_working_state).toBe("working");
      await act(async () => stream.emit("headless", cancelled));
      const bucket = () => useGroupStore.getState().chatByGroup.g!;
      expect(bucket().streamingActivitiesByStreamId["generation:acp-turn"] ?? []).toEqual([]);
      // Buffered progress was flushed, and transient output was retired.
      expect(bucket().streamingEvents).toHaveLength(0);
      expect(useGroupStore.getState().actors[0]!.running).toBe(true);
      expect(useGroupStore.getState().actors[0]!.effective_working_state).toBe("idle");
      expect(useGroupStore.getState().actors[0]!.effective_working_reason).toBe(
        "headless_turn_idle",
      );
    } finally {
      await act(async () => {
        connection!.cleanup();
        root.unmount();
      });
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      host.remove();
    }
  },
);

it.each(
  (["claude", "codex"] as const).flatMap((runtime) =>
    (["idle", "working", "stopped", "error"] as const).map((state) => ({ runtime, state })),
  ),
)(
  "preserves the current $runtime $state state while replaying an old failure",
  async ({ runtime, state }) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const original = useGroupStore.getState();
    const sources = stubEventSources();
    const actor: Actor = {
      id: "native",
      runtime,
      runner: "pty",
      runtime_state_source: "managed_session",
      running: state !== "stopped",
      idle_seconds: state === "idle" ? 5 : null,
      effective_working_state: state,
      effective_working_reason:
        state === "error" ? "A newer provider failure" : "Current daemon state",
      effective_working_updated_at: "2026-10-13T01:00:00Z",
      effective_active_task_id: state === "working" ? "current-turn" : null,
    };
    const group = {
      group_id: "g",
      runtime_status: {
        lifecycle_state: state === "working" ? "active" : state === "stopped" ? "stopped" : "idle",
        runtime_running: actor.running!,
        running_actor_count: actor.running ? 1 : 0,
        has_running_foreman: false,
      },
    };
    useGroupStore.setState({
      selectedGroupId: "g",
      actors: [actor],
      groups: [group],
      groupDoc: null,
      chatByGroup: {},
      refreshActors: vi.fn().mockResolvedValue(undefined),
    });
    let connection: ReturnType<typeof useSSE>;
    function Probe() {
      connection = useSSE({
        activeTabRef: { current: "chat" },
        chatAtBottomRef: { current: true },
        actorsRef: { current: [actor] },
      });
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    const failure: HeadlessStreamEvent = {
      id: "old-failure",
      group_id: "g",
      actor_id: actor.id,
      type: "headless.turn.failed",
      ts: "2026-10-12T01:00:00Z",
      data: {
        turn_id: "old-turn",
        stream_id: "generation:old-turn",
        status: "failed",
        error: "Historical provider failure",
      },
    };
    const frames = [
      {
        ...failure,
        id: "old-thread",
        type: "headless.thread.started",
        data: { thread_id: "thread" },
      },
      { ...failure, id: "old-start", type: "headless.turn.started" },
      failure,
    ];
    try {
      await act(async () => root.render(<Probe />));
      await act(async () => connection!.connectStream("g"));
      const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
      // A successful native turn updates daemon status without adding a success
      // journal entry. The retained failure therefore is not its current state.
      await act(async () => stream.emit("headless.snapshot", { events: frames }));
      expect(useGroupStore.getState().actors[0]).toBe(actor);
      expect(useGroupStore.getState().groups[0]).toBe(group);
      const bucket = () => useGroupStore.getState().chatByGroup.g!;
      expect(bucket().rawHeadlessEventsByActorId.native).toHaveLength(3);
      expect(bucket().streamingActivitiesByStreamId["generation:old-turn"]).toEqual([
        expect.objectContaining({ kind: "error", detail: "Historical provider failure" }),
      ]);
      await act(async () => stream.emit("headless.snapshot", { events: frames }));
      expect(useGroupStore.getState().actors[0]).toBe(actor);
      expect(bucket().rawHeadlessEventsByActorId.native).toHaveLength(3);

      // Fresh live events still update the current runtime and Group projection.
      const fresh = {
        ...failure,
        ts: "2026-10-13T01:00:01Z",
        data: { ...failure.data, turn_id: "fresh-turn", stream_id: "generation:fresh-turn" },
      };
      await act(async () =>
        stream.emit("headless", { ...fresh, id: "fresh-start", type: "headless.turn.started" }),
      );
      expect(useGroupStore.getState().actors[0]!.effective_working_state).toBe("working");
      expect(useGroupStore.getState().groups[0]!.runtime_status!.lifecycle_state).toBe("active");
      await act(async () =>
        stream.emit("headless", {
          ...fresh,
          id: "fresh-failure",
          data: { ...fresh.data, error: "Fresh provider failure" },
        }),
      );
      expect(useGroupStore.getState().actors[0]).toMatchObject({
        running: true,
        effective_working_state: "error",
        effective_working_reason: "Fresh provider failure",
      });
    } finally {
      await act(async () => {
        connection!.cleanup();
        root.unmount();
      });
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      host.remove();
    }
  },
);

it.each([
  { outcome: "session.stopped", state: "stopped", buffered: false },
  { outcome: "session.stopped", state: "stopped", buffered: true },
  { outcome: "turn.cancelled", state: "stopped", buffered: true },
  { outcome: "turn.cancelled", state: "idle", buffered: false },
  { outcome: "turn.completed", state: "idle", buffered: true },
  { outcome: "turn.failed", state: "error", buffered: false },
  { outcome: "turn.cancelled", state: "working", buffered: false },
  { outcome: "turn.completed", state: "working", buffered: true },
  { outcome: "turn.failed", state: "working", buffered: true },
  { outcome: "session.stopped", state: "working", buffered: true },
])(
  "settles $outcome output after its $state snapshot (buffered=$buffered)",
  async ({ outcome, state, buffered }) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const original = useGroupStore.getState();
    const originalUI = useUIStore.getState();
    const sources = stubEventSources();
    const frames = new Map<number, FrameRequestCallback>();
    let frameId = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.set(++frameId, callback);
      return frameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => frames.delete(id));
    const flushFrames = () => {
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(performance.now());
    };
    const actor: Actor = {
      id: "acp",
      runtime: "kimi",
      runtime_mode: "acp",
      runner: "headless",
      runtime_state_source: "managed_session",
      running: true,
      effective_working_state: "working",
      effective_working_updated_at: "2026-10-13T01:00:00Z",
      effective_active_task_id: "turn",
    };
    useGroupStore.setState({
      selectedGroupId: "g",
      actors: [actor],
      groups: [{ group_id: "g", running: true }],
      chatByGroup: {},
      refreshActors: vi.fn().mockResolvedValue(undefined),
    });
    let connection: ReturnType<typeof useSSE>;
    function Probe() {
      connection = useSSE({
        activeTabRef: { current: "chat" },
        chatAtBottomRef: { current: true },
        actorsRef: { current: [actor] },
      });
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    const delta: HeadlessStreamEvent = {
      id: "delta",
      group_id: "g",
      actor_id: actor.id,
      type: "headless.message.delta",
      ts: "2026-10-13T01:00:00.100Z",
      data: { turn_id: "turn", stream_id: "generation:turn", delta: "Partial answer" },
    };
    const terminal: HeadlessStreamEvent = {
      ...delta,
      id: "terminal",
      type: `headless.${outcome}`,
      ts: "2026-10-13T01:00:00.200Z",
      data: outcome === "session.stopped" ? {} : { ...delta.data, error: "Synthetic failure" },
    };
    const bucket = () => useGroupStore.getState().chatByGroup.g!;
    try {
      await act(async () => root.render(<Probe />));
      await act(async () => connection!.connectStream("g"));
      const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
      await act(async () => {
        stream.emit("headless", delta);
        if (!buffered) flushFrames();
      });
      if (!buffered) expect(bucket().streamingEvents[0]!._streaming).toBe(true);
      if (state === "working") {
        // An older turn may still have visible output when the next one starts.
        // Its late terminal must not complete, remove, or flush the new stream.
        await act(async () => {
          useGroupStore
            .getState()
            .reconcileStreamingMessage({
              actorId: actor.id,
              streamId: "generation:next-turn",
              pendingEventId: "next-request",
              ts: "2026-10-13T01:00:00.300Z",
              fullText: "Next answer",
              eventText: "Next answer",
              activities: [],
              completed: false,
              transientStream: false,
              groupId: "g",
            });
          stream.emit("headless", {
            ...delta,
            id: "next-delta",
            ts: "2026-10-13T01:00:00.300Z",
            data: {
              turn_id: "next-turn",
              stream_id: "generation:next-turn",
              event_id: "next-request",
              delta: " still streaming",
            },
          });
        });
      }
      // Actor GET can win the race with the independently delivered terminal.
      // Stopped GET snapshots have no working-state timestamp.
      const snapshot: Actor = {
        ...actor,
        running: state !== "stopped",
        effective_working_state: state,
        effective_working_reason: "Authoritative daemon state",
        effective_working_updated_at: state === "stopped" ? null : "2026-10-13T01:00:00.300Z",
        effective_active_task_id: state === "working" ? "next-turn" : null,
      };
      await act(async () => useGroupStore.getState().setActors([snapshot]));
      const groupBefore = useGroupStore.getState().groups;
      await act(async () => {
        stream.emit("headless", terminal);
        flushFrames();
      });
      expect(useGroupStore.getState().actors[0]).toBe(snapshot);
      expect(useGroupStore.getState().groups).toBe(groupBefore);
      const ignoredOldStop = outcome === "session.stopped" && state === "working";
      expect(
        bucket()
          .streamingEvents.filter(
            (event) => (event.data as { stream_id?: unknown })?.stream_id === "generation:turn",
          )
          .some((event) => event._streaming),
      ).toBe(ignoredOldStop);
      if (state === "working") {
        expect(bucket().streamingTextByStreamId["generation:next-turn"]).toBe(
          "Next answer still streaming",
        );
        expect(bucket().streamingEvents).toContainEqual(
          expect.objectContaining({
            _streaming: true,
            data: expect.objectContaining({ stream_id: "generation:next-turn" }),
          }),
        );
        expect(bucket().replySessionsByPendingEventId["next-request"].phase).toBe("streaming");
      }
      if (outcome === "session.stopped" && !ignoredOldStop) {
        expect(bucket().streamingEvents).toHaveLength(0);
        expect(bucket().streamingTextByStreamId["generation:turn"]).toBeUndefined();
      } else if (outcome === "turn.failed") {
        expect(bucket().streamingActivitiesByStreamId["generation:turn"]).toEqual([
          expect.objectContaining({ kind: "error", detail: "Synthetic failure" }),
        ]);
      } else {
        expect(bucket().streamingTextByStreamId["generation:turn"]).toBe("Partial answer");
      }
      const settled = bucket();
      await act(async () => {
        stream.emit("headless", terminal);
        stream.emit("headless.snapshot", { events: [delta, terminal] });
        flushFrames();
      });
      expect(bucket()).toBe(settled);
      expect(bucket().rawHeadlessEventsByActorId.acp).toHaveLength(state === "working" ? 3 : 2);
      expect(frames.size).toBe(0);
    } finally {
      await act(async () => {
        connection!.cleanup();
        root.unmount();
      });
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      useUIStore.setState(originalUI);
      host.remove();
    }
  },
);

it.each(
  (["claude", "codex"] as const).flatMap((runtime) =>
    (["idle", "working", "stopped", "stopped_without_timestamp", "error"] as const).map(
      (state) => ({ runtime, state }),
    ),
  ),
)(
  "delayed live failure preserves newer $runtime $state state and output",
  async ({ runtime, state }) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const original = useGroupStore.getState();
    const sources = stubEventSources();
    const actor: Actor = {
      id: "native",
      runtime,
      runner: "pty",
      runtime_state_source: "managed_session",
      running: true,
      effective_working_state: "working",
      effective_working_updated_at: "2026-10-13T01:00:00.000Z",
      effective_active_task_id: "failed-turn",
    };
    useGroupStore.setState({
      selectedGroupId: "g",
      groups: [{ group_id: "g", title: "g", running: true }],
      actors: [actor],
      chatByGroup: {},
      refreshActors: vi.fn().mockResolvedValue(undefined),
    });
    let connection: ReturnType<typeof useSSE>;
    function Probe() {
      connection = useSSE({
        activeTabRef: { current: "chat" },
        chatAtBottomRef: { current: true },
        actorsRef: { current: [actor] },
      });
      return null;
    }
    const host = document.createElement("div"),
      root = createRoot(host);
    try {
      await act(async () => root.render(<Probe />));
      await act(async () => connection!.connectStream("g"));
      const ledger = sources.find((source) => source.url.includes("/ledger/stream"))!;
      const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
      await act(async () => stream.emit("headless.snapshot", { events: [] }));
      const recovered = {
        id: "native",
        running: !state.startsWith("stopped"),
        idle_seconds: null,
        effective_working_state: state.startsWith("stopped") ? "stopped" : state,
        effective_working_reason: "newer daemon state",
        effective_working_updated_at:
          state === "stopped_without_timestamp" ? null : "2026-10-13T01:00:00.150Z",
        effective_active_task_id: state === "working" ? "next-turn" : null,
      };
      await act(async () =>
        ledger.emit("ledger", {
          id: "recovery",
          group_id: "g",
          kind: "actor.activity",
          ts: "2026-10-13T01:00:00.200Z",
          data: { actors: [recovered] },
        }),
      );
      expect(useGroupStore.getState().actors[0]).toMatchObject(recovered);
      if (state === "working") {
        // A subsequent turn's placeholder must survive the previous turn's late terminal.
        useGroupStore
          .getState()
          .reconcileStreamingMessage({
            actorId: "native",
            streamId: "generation:next-turn",
            ts: recovered.effective_working_updated_at || "2026-10-13T01:00:00.150Z",
            fullText: "New turn output",
            eventText: "New turn output",
            activities: [],
            completed: false,
            transientStream: false,
            groupId: "g",
          });
      }
      const outputBefore = useGroupStore.getState().chatByGroup.g?.streamingEvents || [];
      const runtimeBefore = useGroupStore.getState().groups;
      await act(async () =>
        stream.emit("headless", {
          id: "unseen-live-failure",
          group_id: "g",
          actor_id: "native",
          type: "headless.turn.failed",
          ts: state === "working" ? "2026-10-13T01:00:00.175Z" : "2026-10-13T01:00:00.100Z",
          data: {
            turn_id: "failed-turn",
            stream_id: "generation:failed-turn",
            status: "failed",
            error: "Synthetic old failure",
          },
        }),
      );
      expect(useGroupStore.getState().actors[0]).toMatchObject(recovered);
      expect(useGroupStore.getState().chatByGroup.g?.streamingEvents || []).toEqual(outputBefore);
      expect(useGroupStore.getState().groups).toEqual(runtimeBefore);
      expect(
        useGroupStore.getState().chatByGroup.g.rawHeadlessEventsByActorId.native,
      ).toContainEqual(expect.objectContaining({ id: "unseen-live-failure" }));
      if (state === "working") {
        // A valid output delta can predate the latest activity tick.
        await act(async () => {
          stream.emit("headless", {
            id: "delayed-output",
            group_id: "g",
            actor_id: "native",
            type: "headless.message.delta",
            ts: "2026-10-13T01:00:00.140Z",
            data: {
              turn_id: "next-turn",
              stream_id: "generation:next-turn",
              delta: " still arrives",
            },
          });
          await new Promise((resolve) => setTimeout(resolve, 35));
        });
        expect(
          useGroupStore.getState().chatByGroup.g.streamingTextByStreamId["generation:next-turn"],
        ).toBe("New turn output still arrives");
      }
    } finally {
      await act(async () => {
        connection!.cleanup();
        root.unmount();
      });
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      host.remove();
    }
  },
);
