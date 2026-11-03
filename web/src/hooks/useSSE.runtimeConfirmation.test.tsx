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

it.each([
  "headless.session.started",
  "headless.session.stopped",
  "headless.session.exited",
  "headless.turn.failed",
])("refreshes a confirmation PTY on %s, but not on ordinary output", async (eventType) => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const original = useGroupStore.getState();
  const actor: Actor = {
    id: "claude",
    runtime: "claude",
    runner: "pty",
    running: true,
    runtime_state_source: "terminal",
    runtime_confirmation_required: true,
  };
  useGroupStore.setState({ selectedGroupId: "g", actors: [actor], chatByGroup: {} });
  const admitted = eventType === "headless.session.started";
  const fetchActors = vi
    .spyOn(api, "fetchActors")
    .mockResolvedValue({
      ok: true,
      result: {
        actors: [
          {
            ...actor,
            runtime_confirmation_required: false,
            running: admitted,
            runtime_state_source: admitted ? "managed_session" : "terminal",
          },
        ],
      },
    });
  class Stream extends EventTarget {
    onopen: (() => void) | null = null;
    onerror: (() => void) | null = null;
    close() {}
    emit(id: string, type: string) {
      this.dispatchEvent(
        new MessageEvent("headless", {
          data: JSON.stringify({
            id,
            type,
            group_id: "g",
            actor_id: "claude",
            ts: "2026-11-02T10:00:00Z",
            data: { stream_id: "s", delta: "Fixture output" },
          }),
        }),
      );
    }
  }
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
    const stream = sources.find((source) => source.path.includes("/headless/stream"))!.stream;
    await act(async () => stream.emit("delta", "headless.message.delta"));
    expect(fetchActors).not.toHaveBeenCalled();
    await act(async () => stream.emit("lifecycle", eventType));
    expect(fetchActors).toHaveBeenCalledOnce();
    expect(useGroupStore.getState().actors[0]).toMatchObject({
      runtime_confirmation_required: false,
      running: admitted,
    });
    await act(async () => stream.emit("later-delta", "headless.message.delta"));
    expect(fetchActors).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState(original, true);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    network.open.mockReset();
  }
});
