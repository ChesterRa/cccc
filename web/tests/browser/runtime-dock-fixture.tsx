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
const sockets: FixtureSocket[] = [];
class FixtureSocket {
  static OPEN = 1;
  readyState = 0;
  subscriptions = new Map<string, number>();
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public url: string) {
    sockets.push(this);
    setTimeout(() => {
      this.readyState = FixtureSocket.OPEN;
      this.onopen?.();
    }, 0);
  }
  send(text: string) {
    const command = JSON.parse(text);
    if (command.type === "subscribe") {
      this.subscriptions.set(command.channel, command.id);
      queueMicrotask(() =>
        this.onmessage?.({
          data: JSON.stringify({ type: "ready", channel: command.channel, id: command.id }),
        }),
      );
    } else if (command.type === "unsubscribe") {
      this.subscriptions.delete(command.channel);
    }
  }
  close() {
    this.readyState = 3;
    this.subscriptions.clear();
  }
  emit(type: string, value: unknown) {
    this.onmessage?.({
      data: JSON.stringify({
        type: "event",
        channel: "headless",
        id: this.subscriptions.get("headless"),
        message: { event: type, data: value },
      }),
    });
  }
}
Object.assign(window, { WebSocket: FixtureSocket });
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
  [...sockets]
    .reverse()
    .find(
      (socket) => socket.readyState === FixtureSocket.OPEN && socket.subscriptions.has("headless"),
    )!;
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
