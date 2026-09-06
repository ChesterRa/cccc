import { useEffect } from "react";
import "../../src/index.css";
import { RuntimeDock } from "../../src/pages/chat/RuntimeDock";
import { useSSE } from "../../src/hooks/useSSE";
import { useGroupStore } from "../../src/stores/useGroupStore";
import { buildLiveWorkCards } from "../../src/pages/chat/liveWorkCards";
import type { Actor, HeadlessStreamEvent } from "../../src/types";

const actors: Actor[] = ["claude", "codex", "grok", "opencode", "kilo"].map((runtime, i) => ({
  id: `actor-${i}`,
  title: `${runtime} ${i + 1}`,
  runtime,
  runner: "pty",
  runtime_state_source: "managed_session",
  running: true,
  enabled: true,
  effective_working_state: "working",
}));
const sources: FakeEventSource[] = [];
class FakeEventSource {
  listeners = new Map<string, EventListener[]>();
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  constructor(public url: string) {
    sources.push(this);
    setTimeout(() => this.onopen?.(), 0);
  }
  addEventListener(type: string, listener: EventListener) {
    this.listeners.set(type, [...(this.listeners.get(type) || []), listener]);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, value: unknown) {
    for (const listener of this.listeners.get(type) || [])
      listener(new MessageEvent(type, { data: JSON.stringify(value) }));
  }
}
Object.assign(window, { EventSource: FakeEventSource });
window.fetch = async () =>
  new Response(JSON.stringify({ ok: true, result: {} }), {
    headers: { "Content-Type": "application/json" },
  });
useGroupStore.setState({
  selectedGroupId: "fixture-a",
  actors,
  chatByGroup: {},
  refreshActors: async () => {},
  refreshPresentation: async () => {},
});
let connection: ReturnType<typeof useSSE>;
const options = {
  activeTabRef: { current: "chat" },
  chatAtBottomRef: { current: true },
  actorsRef: { current: actors },
};
const stream = () =>
  [...sources]
    .reverse()
    .find((source) => !source.closed && source.url.includes("/headless/stream"))!;
const frames = (
  groupId: string,
  actorId: string,
  id: string,
  text: string,
): HeadlessStreamEvent[] => [
  {
    id: `${id}-start`,
    group_id: groupId,
    actor_id: actorId,
    type: "headless.control.started",
    ts: "2026-09-04T00:00:00Z",
    data: { turn_id: id },
  },
  {
    id: `${id}-delta`,
    group_id: groupId,
    actor_id: actorId,
    type: "headless.message.delta",
    ts: "2026-09-04T00:00:01Z",
    data: { turn_id: id, stream_id: id, delta: text },
  },
  {
    id: `${id}-done`,
    group_id: groupId,
    actor_id: actorId,
    type: "headless.message.completed",
    ts: "2026-09-04T00:00:02Z",
    data: { turn_id: id, stream_id: id, text },
  },
  {
    id: `${id}-end`,
    group_id: groupId,
    actor_id: actorId,
    type: "headless.control.completed",
    ts: "2026-09-04T00:00:03Z",
    data: { turn_id: id, status: "completed" },
  },
];
export function Fixture() {
  const state = useGroupStore();
  connection = useSSE(options);
  useEffect(() => {
    connection.connectStream(state.selectedGroupId);
    return connection.cleanup;
  }, [state.selectedGroupId]);
  const bucket = state.chatByGroup[state.selectedGroupId];
  const cards = buildLiveWorkCards({
    actors,
    events: bucket?.streamingEvents || [],
    latestActorPreviewByActorId: bucket?.latestActorPreviewByActorId || {},
    previewSessionsByActorId: bucket?.previewSessionsByActorId || {},
    latestActorTextByActorId: bucket?.latestActorTextByActorId || {},
    latestActorActivitiesByActorId: bucket?.latestActorActivitiesByActorId || {},
    replySessionsByPendingEventId: bucket?.replySessionsByPendingEventId || {},
  });
  const dark = document.documentElement.classList.contains("dark");
  return (
    <main
      style={{
        height: "100vh",
        background: dark ? "#0c0d10" : "#fff",
        color: dark ? "#eee" : "#222",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div style={{ padding: 16 }}>Runtime progress fixture</div>
      <div style={{ flex: 1, position: "relative", minHeight: 0 }}>
        <div style={{ position: "absolute", bottom: 0, width: "100%" }}>
          <RuntimeDock
            groupId={state.selectedGroupId}
            runtimeActors={actors}
            liveWorkCards={cards}
            runtimeEvents={bucket?.rawHeadlessEventsByActorId}
            isDark={dark}
            isSmallScreen={innerWidth < 640}
            actorStatusProvisional={false}
            onOpenRuntimeActor={() => {}}
          />
        </div>
      </div>
      <input
        aria-label="Message"
        placeholder="Message"
        style={{ margin: 12, padding: 12, border: "1px solid #aaa", borderRadius: 8 }}
      />
    </main>
  );
}
Object.assign(window, {
  dockFixture: {
    ready: () => Boolean(stream()),
    restore: () =>
      stream().emit("headless.snapshot", {
        events: frames(
          useGroupStore.getState().selectedGroupId,
          "actor-0",
          "old",
          "Old work.\n".repeat(80) + "Already completed yesterday.",
        ),
      }),
    replay: async () => {
      for (const event of frames(
        useGroupStore.getState().selectedGroupId,
        "actor-0",
        "old",
        "Old work.\n".repeat(80) + "Already completed yesterday.",
      )) {
        stream().emit("headless", event);
        await new Promise((r) => setTimeout(r, 55));
      }
    },
    live: (actorId: string, id: string, text: string) => {
      for (const event of frames(useGroupStore.getState().selectedGroupId, actorId, id, text))
        stream().emit("headless", event);
    },
    group: (id: string) => useGroupStore.setState({ selectedGroupId: id }),
    dark: (enabled: boolean) => {
      document.documentElement.classList.toggle("dark", enabled);
      useGroupStore.setState({ actors: [...actors] });
    },
    texts: () =>
      Array.from(document.querySelectorAll(".runtime-dock-ticker-entry")).map(
        (node) => node.textContent,
      ),
  },
});
