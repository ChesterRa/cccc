// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useSSE } from "./useSSE";
import { useGroupStore } from "../stores";
import { HeadlessRuntimePanel } from "../components/headless/HeadlessRuntimePanel";
import { projectUnboundTrace } from "../components/headless/unboundTraceProjection";
import { buildHeadlessPreviewTimelineEntries } from "../pages/chat/headlessPreviewTimeline";
import type { Actor, HeadlessStreamEvent } from "../types";

vi.mock("../services/realtime/eventStream", () => ({
  openEventStream: (url: string) => new EventSource(url),
}));
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) => opts?.defaultValue || key,
    i18n: { language: "en" },
  }),
}));

const actor = (id: string): Actor => ({
  id,
  runtime: "pi",
  runner: "headless",
  runtime_state_source: "managed_session",
  running: true,
});

function event(
  id: string,
  type: string,
  data: Record<string, unknown>,
  options: { actor?: string; group?: string; stream?: string; ts?: string } = {},
): HeadlessStreamEvent {
  return {
    id,
    group_id: options.group ?? "g",
    actor_id: options.actor ?? "a",
    type,
    ts: options.ts ?? "2026-01-01T00:00:01Z",
    data: { stream_id: options.stream ?? "s", ...data },
  };
}

/** Mount one real SSE hook with a disposable stream and store for each scenario. */
async function openFixture(actors: Actor[] = [actor("a")]) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const original = useGroupStore.getState();
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
  useGroupStore.setState({ selectedGroupId: "g", actors, chatByGroup: {} });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: true },
      actorsRef: { current: actors },
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Probe />));
  await act(async () => connection!.connectStream("g"));
  const stream = sources.find((source) => source.url.includes("/headless/stream"))!;
  return {
    stream,
    async dispose() {
      await act(async () => {
        connection!.cleanup();
        root.unmount();
      });
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      host.remove();
    },
  };
}

it.each([
  ["replay", "tool-first"],
  ["replay", "text-first"],
  ["live", "tool-first"],
  ["live", "text-first"],
])("keeps the tool visible in %s: %s", async (delivery, order) => {
  const fixture = await openFixture();
  const panelHost = document.createElement("div");
  document.body.append(panelHost);
  const panel = createRoot(panelHost);
  const delta = event("delta", "headless.message.delta", { delta: "Message text" });
  const activity = event(
    "activity",
    "headless.activity.completed",
    {
      activity_id: "tool",
      kind: "tool",
      summary: "bash failed",
      tool_name: "bash",
      command: "cargo test",
    },
    { ts: "2026-01-01T00:00:02Z" },
  );
  const events =
    order === "text-first"
      ? [delta, activity]
      : [
          { ...activity, ts: "2026-01-01T00:00:01Z" },
          { ...delta, ts: "2026-01-01T00:00:02Z" },
        ];
  try {
    if (delivery === "replay") {
      await act(async () => fixture.stream.emit("headless.snapshot", { events }));
      await act(async () => fixture.stream.emit("headless.snapshot", { events }));
      await act(async () => fixture.stream.emit("headless", events[0]));
    } else {
      for (const item of events)
        await act(async () => {
          fixture.stream.emit("headless", item);
          await new Promise((resolve) => setTimeout(resolve, 30));
        });
    }
    const bucket = useGroupStore.getState().chatByGroup.g!;
    expect(bucket.rawHeadlessEventsByActorId.a).toHaveLength(2);
    const row = bucket.streamingEvents.find((item) => item.by === "a")!;
    expect((row.data as { text?: string }).text).toBe("Message text");
    expect(
      (row.data as { activities?: { id: string }[] }).activities?.map((item) => item.id),
    ).toEqual(["tool"]);
    expect((row.data as { _headless_text_started_at?: string })._headless_text_started_at).toBe(
      order === "text-first" ? "2026-01-01T00:00:01Z" : "2026-01-01T00:00:02Z",
    );
    await act(async () =>
      panel.render(
        <HeadlessRuntimePanel
          actorId="a"
          groupId="g"
          streamingEvents={bucket.streamingEvents}
          previewSessions={bucket.previewSessionsByActorId.a || []}
          fallbackText={bucket.latestActorTextByActorId.a || ""}
          fallbackActivities={bucket.latestActorActivitiesByActorId.a || []}
          rawEvents={bucket.rawHeadlessEventsByActorId.a || []}
          emptyLabel="Empty"
          isDark={false}
          compact
        />,
      ),
    );
    expect(panelHost.textContent).toContain("Message text");
    expect(panelHost.textContent).toContain("bash failed");
    const visible =
      panelHost.querySelector("[data-headless-trace]")?.lastElementChild?.textContent || "";
    expect(visible.indexOf("Message text") < visible.indexOf("bash failed")).toBe(
      order === "text-first",
    );
    const projected = projectUnboundTrace("g", "a", bucket.streamingEvents, []);
    expect(projected).not.toBeNull();
    expect(
      buildHeadlessPreviewTimelineEntries({ previewSessions: [projected!] }).map(
        (item) => item.kind,
      ),
    ).toEqual(order === "tool-first" ? ["activity", "message"] : ["message", "activity"]);
  } finally {
    await act(async () => panel.unmount());
    panelHost.remove();
    await fixture.dispose();
  }
});

it.each([
  [50, 0, false],
  [51, 0, false],
  [51, 60, true],
])(
  "retains %i pre-text activities and %i later activities from replay",
  async (count, afterCount, overlap) => {
    const fixture = await openFixture();
    try {
      const activities = Array.from({ length: count }, (_, i) =>
        event(`event-${i}`, "headless.activity.completed", {
          activity_id: `tool-${i}`,
          kind: "tool",
          summary: `call ${i}`,
        }),
      );
      const text = event("text", "headless.message.delta", { delta: "Hello" });
      const later = Array.from({ length: afterCount }, (_, i) =>
        event(`later-${i}`, "headless.activity.completed", {
          activity_id: `later-tool-${i}`,
          kind: "tool",
          summary: `later ${i}`,
        }),
      );
      if (overlap)
        later.unshift(
          event("overlap", "headless.activity.completed", {
            activity_id: "tool-0",
            kind: "tool",
            summary: "finished later",
          }),
        );
      await act(async () =>
        fixture.stream.emit("headless.snapshot", { events: [...activities, text, ...later] }),
      );
      const expected = [
        ...Array.from({ length: count }, (_, i) => `tool-${i}`),
        ...Array.from({ length: afterCount }, (_, i) => `later-tool-${i}`),
      ];
      const read = () =>
        useGroupStore
          .getState()
          .chatByGroup.g!.streamingEvents.find(
            (item) => (item.data as { stream_id?: string })?.stream_id === "s",
          )!;
      expect(
        (read().data as { activities: { id: string }[] }).activities.map((item) => item.id),
      ).toEqual(expected);
      if (overlap)
        expect(
          (read().data as { activities: { id: string; summary: string }[] }).activities.find(
            (item) => item.id === "tool-0",
          )?.summary,
        ).toBe("finished later");
      await act(async () => {
        for (let i = 0; i < 405; i++)
          fixture.stream.emit(
            "headless",
            event(`tail-${i}`, "headless.message.delta", { delta: "x" }),
          );
        await new Promise((resolve) => setTimeout(resolve, 30));
      });
      expect(useGroupStore.getState().chatByGroup.g!.rawHeadlessEventsByActorId.a).toHaveLength(
        400,
      );
      expect(
        (read().data as { activities: { id: string }[] }).activities.map((item) => item.id),
      ).toEqual(expected);
      await act(async () => {
        fixture.stream.emit(
          "headless",
          event("update", "headless.activity.completed", {
            activity_id: `tool-${count - 1}`,
            kind: "tool",
            summary: "updated",
          }),
        );
        await new Promise((resolve) => setTimeout(resolve, 30));
      });
      const updated = (read().data as { activities: { id: string; summary: string }[] }).activities;
      expect(updated).toHaveLength(expected.length);
      expect(updated.find((item) => item.id === `tool-${count - 1}`)?.summary).toBe("updated");
    } finally {
      await fixture.dispose();
    }
  },
);

it("matches activities to group, actor and stream in replay and keeps them after the raw window advances", async () => {
  const fixture = await openFixture([actor("a"), actor("b")]);
  const tool = (summary: string) => ({ activity_id: "same-tool-id", kind: "tool", summary });
  const opts = (actorId: string, stream: string, group = "g") => ({
    actor: actorId,
    stream,
    group,
  });
  const events = [
    event("no-stream", "headless.activity.completed", tool("no-stream-tool"), opts("a", "")),
    event("1", "headless.activity.completed", tool("alpha-tool"), opts("a", "a-1")),
    event("2", "headless.activity.completed", tool("beta-tool"), opts("b", "b-1")),
    event("3", "headless.activity.completed", tool("second-tool"), opts("a", "a-2")),
    event("4", "headless.activity.completed", tool("wrong-group"), opts("a", "a-1", "other")),
    event("5", "headless.message.delta", { delta: "Second" }, opts("a", "a-2")),
    event("6", "headless.message.delta", { delta: "Beta" }, opts("b", "b-1")),
    event("7", "headless.message.delta", { delta: "Alpha" }, opts("a", "a-1")),
  ];
  try {
    await act(async () => fixture.stream.emit("headless.snapshot", { events }));
    const bucket = useGroupStore.getState().chatByGroup.g!;
    for (const [actorId, stream, text, summary] of [
      ["a", "a-1", "Alpha", "alpha-tool"],
      ["a", "a-2", "Second", "second-tool"],
      ["b", "b-1", "Beta", "beta-tool"],
    ]) {
      const row = bucket.streamingEvents.find(
        (item) =>
          item.by === actorId && (item.data as { stream_id?: string })?.stream_id === stream,
      )!;
      expect((row.data as { text?: string }).text).toBe(text);
      expect(
        (row.data as { activities?: { summary: string }[] }).activities?.map((a) => a.summary),
      ).toEqual([summary]);
    }
    await act(async () => {
      fixture.stream.emit(
        "headless",
        event("8", "headless.activity.completed", tool("alpha-tool revised"), opts("a", "a-1")),
      );
      fixture.stream.emit(
        "headless",
        event("9", "headless.turn.completed", { turn_id: "turn-a" }, opts("a", "a-1")),
      );
    });
    const after = useGroupStore
      .getState()
      .chatByGroup.g!.streamingEvents.find(
        (item) => item.by === "a" && (item.data as { stream_id?: string })?.stream_id === "a-1",
      )!;
    expect(
      (after.data as { activities?: { summary: string }[] }).activities?.map((a) => a.summary),
    ).toEqual(["alpha-tool revised"]);
    await act(async () => {
      fixture.stream.emit(
        "headless",
        event("long-tool-event", "headless.activity.completed", tool("long-tool"), {
          stream: "long",
        }),
      );
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    await act(async () => {
      fixture.stream.emit(
        "headless",
        event(
          "long-first-text",
          "headless.message.delta",
          { delta: "first" },
          { stream: "long", ts: "2026-01-01T00:00:02Z" },
        ),
      );
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    await act(async () => {
      for (let i = 0; i < 405; i++)
        fixture.stream.emit(
          "headless",
          event(
            `long-tail-${i}`,
            "headless.message.delta",
            { delta: "x" },
            { stream: "long", ts: "2026-01-01T00:00:03Z" },
          ),
        );
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    const current = useGroupStore.getState().chatByGroup.g!;
    expect(current.rawHeadlessEventsByActorId.a).toHaveLength(400);
    expect(
      current.rawHeadlessEventsByActorId.a.some(
        (item) => item.id === "long-tool-event" || item.id === "long-first-text",
      ),
    ).toBe(false);
    const projected = projectUnboundTrace("g", "a", current.streamingEvents, [])!;
    expect(projected.currentStreamId).toBe("long");
    expect(projected.latestText).toBe(`first${"x".repeat(405)}`);
    expect(projected.transcriptBlocks[0].updatedAt).toBe("2026-01-01T00:00:02Z");
    expect(projected.activities.map((item) => item.summary)).toEqual(["long-tool"]);
    expect(
      buildHeadlessPreviewTimelineEntries({ previewSessions: [projected] }).map(
        (item) => item.kind,
      ),
    ).toEqual(["activity", "message"]);
  } finally {
    await fixture.dispose();
  }
});

it.each(["snapshot", "live"])(
  "does not leave one synthetic chat row per textless Pi turn in %s",
  async (delivery) => {
    const fixture = await openFixture();
    const events: HeadlessStreamEvent[] = [];
    for (let i = 0; i < 100; i++) {
      const stream = `generation:turn-${i}`,
        ts = new Date(Date.UTC(2026, 0, 1, 0, 0, i)).toISOString();
      const mk = (type: string, data: Record<string, unknown>) =>
        event(`${i}:${type}`, type, data, { stream, ts });
      events.push(
        mk("headless.activity.completed", {
          activity_id: `tool-${i}`,
          kind: "tool",
          summary: "bash",
        }),
      );
      events.push(mk("headless.message.completed", {}));
      events.push(mk("headless.turn.completed", { turn_id: `turn-${i}` }));
    }
    try {
      if (delivery === "snapshot")
        await act(async () => fixture.stream.emit("headless.snapshot", { events }));
      else
        await act(async () => {
          for (const item of events) fixture.stream.emit("headless", item);
        });
      const bucket = useGroupStore.getState().chatByGroup.g!;
      expect(bucket.rawHeadlessEventsByActorId.a).toHaveLength(300);
      expect(bucket.streamingEvents).toHaveLength(0);
      expect(Object.keys(bucket.streamingActivitiesByStreamId)).toHaveLength(0);
    } finally {
      await fixture.dispose();
    }
  },
);
