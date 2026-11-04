// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useGroupStore } from "../stores";
import type { Actor } from "../types";

const network = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("../services/realtime/eventStream", () => ({ openEventStream: network.open }));
import { useSSE } from "./useSSE";

class Stream extends EventTarget {
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close() {}
  emit(id: string, type: string, data: object = { stream_id: "s", delta: "Fixture output" }) {
    this.dispatchEvent(
      new MessageEvent("headless", {
        data: JSON.stringify({
          id,
          type,
          group_id: "g",
          actor_id: "claude",
          ts: "2026-11-02T10:00:00Z",
          data,
        }),
      }),
    );
  }
}

/** Connects the hook to a fake Group stream whose Actor refresh returns `refreshed`. */
async function withStream(actor: Actor, refreshed: Actor, run: (stream: Stream) => Promise<void>) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const original = useGroupStore.getState();
  useGroupStore.setState({ selectedGroupId: "g", actors: [actor], chatByGroup: {} });
  vi.spyOn(api, "fetchActors").mockResolvedValue({ ok: true, result: { actors: [refreshed] } });
  const sources: { path: string; stream: Stream }[] = [];
  network.open.mockImplementation((path: string) => {
    const stream = new Stream();
    sources.push({ path, stream });
    return stream;
  });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: actor.id },
      chatAtBottomRef: { current: true },
      actorsRef: { current: [actor] },
    });
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => connection.connectStream("g"));
    await run(sources.find((source) => source.path.includes("/headless/stream"))!.stream);
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState(original, true);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    network.open.mockReset();
  }
}

it.each([
  "headless.session.started",
  "headless.session.stopped",
  "headless.session.exited",
  "headless.turn.failed",
])("refreshes a confirmation PTY on %s, but not on ordinary output", async (eventType) => {
  const actor: Actor = {
    id: "claude",
    runtime: "claude",
    runner: "pty",
    running: true,
    runtime_state_source: "terminal",
    runtime_confirmation_required: true,
  };
  const admitted = eventType === "headless.session.started";
  const refreshed: Actor = {
    ...actor,
    runtime_confirmation_required: false,
    running: admitted,
    runtime_state_source: admitted ? "managed_session" : "terminal",
  };
  await withStream(actor, refreshed, async (stream) => {
    await act(async () => stream.emit("delta", "headless.message.delta"));
    expect(api.fetchActors).not.toHaveBeenCalled();
    await act(async () => stream.emit("lifecycle", eventType));
    expect(api.fetchActors).toHaveBeenCalledOnce();
    expect(useGroupStore.getState().actors[0]).toMatchObject({
      runtime_confirmation_required: false,
      running: admitted,
    });
    await act(async () => stream.emit("later-delta", "headless.message.delta"));
    expect(api.fetchActors).toHaveBeenCalledOnce();
  });
});

it("reads the pause after a confirmation that closed before the page saw it", async () => {
  // A stopped Actor with a known state time, so only the closed confirmation asks for a refresh.
  const actor: Actor = {
    id: "claude",
    runtime: "claude",
    runner: "pty",
    running: false,
    effective_working_updated_at: "2026-11-02T09:00:00Z",
  };
  await withStream(actor, { ...actor, runtime_confirmation_paused: true }, async (stream) => {
    await act(async () => stream.emit("ordinary-exit", "headless.session.exited", {}));
    expect(api.fetchActors).not.toHaveBeenCalled();
    await act(async () =>
      stream.emit("closed", "headless.session.exited", {
        reason: "interactive_confirmation_ended",
      }),
    );
    expect(api.fetchActors).toHaveBeenCalledOnce();
    expect(useGroupStore.getState().actors[0]?.runtime_confirmation_paused).toBe(true);
  });
});
