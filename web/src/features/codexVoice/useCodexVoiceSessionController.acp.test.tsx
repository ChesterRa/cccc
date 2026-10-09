// @vitest-environment happy-dom
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceAnalystInfo } from "../../services/api/codexVoice";
import { CodexVoiceAnalystPane } from "./CodexVoiceConsolePanes";
import { useCodexVoiceSessionController } from "./useCodexVoiceSessionController";

const fixture = vi.hoisted(() => ({
  analyst: null as CodexVoiceAnalystInfo | null,
  active: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
  control: vi.fn(),
  capture: vi.fn(async (_device: string) => ({ getTracks: () => [], getAudioTracks: () => [] })),
  t: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: fixture.t }) }));
vi.mock("../../services/api", () => ({
  fetchActiveCodexVoiceCall: fixture.active,
  startCodexVoiceCall: fixture.start,
  stopCodexVoiceCall: fixture.stop,
  prepareCodexVoiceNotificationOutput: vi.fn(),
  cancelCodexVoiceAnalyst: vi.fn(),
  resetCodexVoiceAnalyst: vi.fn(),
  getCodexVoiceWebSocketUrl: () => "ws://isolated-fixture",
}));
vi.mock("../../services/api/codexVoice", () => ({
  fetchActiveCodexVoiceCall: fixture.active,
  controlCodexVoiceAnalyst: fixture.control,
}));
vi.mock("./codexVoiceMedia", () => ({
  applyOutputDevice: async () => {},
  captureMicrophone: fixture.capture,
  createClientSessionId: () => "isolated-client",
  waitForIceGathering: async () => {},
  waitForDataChannelOpen: async () => {},
}));

afterEach(() => {
  useVoiceAudioStore.getState().update({ inputDeviceId: "", outputDeviceId: "" });
  fixture.capture.mockClear();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("keeps cancellation available while the next owned-call input awaits ACP admission", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn() } });
  const wire = { readyState: "open", send: vi.fn(), close: vi.fn() };
  vi.stubGlobal(
    "RTCPeerConnection",
    class {
      localDescription = { sdp: "fixture-offer" };
      addTrack() {}
      createDataChannel() {
        return wire;
      }
      async createOffer() {
        return { type: "offer", sdp: "fixture-offer" };
      }
      async setLocalDescription() {}
      async setRemoteDescription() {}
      close() {}
    },
  );
  const socket = {
    onmessage: null as ((event: { data: string }) => void) | null,
    send: vi.fn(),
    close: vi.fn(),
    readyState: 1,
  };
  vi.stubGlobal(
    "WebSocket",
    class {
      static OPEN = 1;
      constructor() {
        return socket;
      }
    },
  );
  const call = {
    generation: "owned-call",
    analyst_generation: "acp-analyst",
    mode: "assistant",
    connected: false,
    voice: "cove",
  };
  fixture.analyst = {
    generation: "acp-analyst",
    structured: true,
    tui_ready: false,
    phase: "working",
    last_result: "",
    warning: "",
    queued_inputs: 0,
    manual_tasks: [],
  };
  fixture.active.mockImplementation(async () => ({
    ok: true,
    result: {
      call: null,
      analyst: fixture.analyst,
      voices: ["cove"],
      readiness: {
        analyst_runtime: "copilot",
        analyst_runtime_available: true,
        realtime_credentials_available: true,
      },
    },
  }));
  fixture.start.mockResolvedValue({
    ok: true,
    result: { call, analyst: fixture.analyst, answer_sdp: "fixture-answer" },
  });
  fixture.stop.mockResolvedValue({ ok: true, result: { stopped: true } });
  fixture.control.mockImplementation(async (_generation, command) => {
    fixture.analyst = {
      ...fixture.analyst!,
      queued_inputs: 1,
      manual_tasks: [
        {
          id: command.input_id,
          text: command.text,
          call_generation: call.generation,
          status: "queued",
          result: "",
          error: "",
        },
      ],
    };
    return { ok: true, result: { accepted: true, analyst: fixture.analyst } };
  });
  let current!: ReturnType<typeof useCodexVoiceSessionController>;
  function App() {
    const controller = useCodexVoiceSessionController();
    current = controller;
    return (
      <>
        <audio ref={controller.audioRef} />
        <button onClick={() => void controller.start()}>start</button>
        <CodexVoiceAnalystPane controller={controller} analystPhase="" visible terminalVisible />
      </>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const cancel = () =>
    [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "actors:acpControls.cancel",
    );
  useVoiceAudioStore.getState().update({ inputDeviceId: "mic-a", outputDeviceId: "speaker-a" });
  try {
    await act(async () => root.render(<App />));
    await act(async () => host.querySelector("button")!.click());
    expect(fixture.start, current.error).toHaveBeenCalledOnce();
    expect(socket.onmessage, current.error).toBeTypeOf("function");
    await act(async () =>
      socket.onmessage?.({
        data: JSON.stringify({ type: "ready", call: { ...call, connected: true } }),
      }),
    );
    expect(current.owned).toBe(true);
    expect(fixture.capture).toHaveBeenCalledWith("mic-a");
    await act(async () =>
      current.updatePreferences({ inputDeviceId: "mic-b", outputDeviceId: "speaker-b" }),
    );
    expect(current.preferences.inputDeviceId).toBe("mic-b");
    expect(current.audioDeviceSnapshot).toEqual({
      inputDeviceId: "mic-a",
      outputDeviceId: "speaker-a",
    });
    expect(current.call?.connected).toBe(true);
    const field = host.querySelector("textarea")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        field,
        "second investigation",
      );
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      host
        .querySelector("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(fixture.control).toHaveBeenCalledOnce();
    // The previous turn finishes. The queue has moved its next input into an
    // admission wait, so queued_inputs is zero but that task remains pending.
    await act(async () =>
      socket.onmessage?.({ data: JSON.stringify({ type: "analyst_terminal" }) }),
    );
    fixture.analyst = { ...fixture.analyst!, phase: "ready", queued_inputs: 0 };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("second investigation");
    expect(cancel(), "pending admission must remain cancellable").toBeDefined();
    await act(async () => cancel()!.click());
    expect(socket.send.mock.calls.map(([frame]) => JSON.parse(frame))).toContainEqual({
      type: "cancel_current",
    });
    fixture.analyst = {
      ...fixture.analyst!,
      manual_tasks: fixture.analyst!.manual_tasks!.map((task) => ({
        ...task,
        status: "cancelled",
      })),
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(cancel()).toBeUndefined();
    await act(async () =>
      current.updateAnalystSnapshot({
        ...fixture.analyst!,
        generation: "retired-analyst",
        phase: "working",
      }),
    );
    expect(current.analyst?.generation).toBe("acp-analyst");
    expect(cancel()).toBeUndefined();
    await act(async () => current.disconnect());
    await act(async () => {
      void current.start();
    });
    await act(async () =>
      socket.onmessage?.({
        data: JSON.stringify({ type: "ready", call: { ...call, connected: true } }),
      }),
    );
    expect(fixture.capture).toHaveBeenLastCalledWith("mic-b");
    expect(current.audioDeviceSnapshot).toEqual({
      inputDeviceId: "mic-b",
      outputDeviceId: "speaker-b",
    });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
