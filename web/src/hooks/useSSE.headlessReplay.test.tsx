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
