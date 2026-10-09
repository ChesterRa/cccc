// @vitest-environment happy-dom
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceAnalystInfo } from "../../services/api/codexVoice";
import { CodexVoiceAnalystModal } from "../../components/modals/CodexVoiceAnalystModal";
vi.mock("./CodexVoiceMessageSources", () => ({ CodexVoiceMessageSources: () => null }));
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

it("surfaces a new ACP approval while the owned-call Analyst is collapsed", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn() } });
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
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
        <CodexVoiceAnalystModal
          isOpen
          isDark={false}
          isSmallScreen={false}
          controller={controller}
          onClose={() => {}}
        />
      </>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
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

    const reads = fixture.active.mock.calls.length;
    fixture.analyst = {
      ...fixture.analyst!,
      permissions: [
        { request_id: "task-token", title: "Confirm test search", kind: "search", details: {} },
      ],
    };
    await act(async () => vi.advanceTimersByTimeAsync(6000));
    expect(fixture.active.mock.calls.length).toBeGreaterThan(reads);
    expect(host.querySelector("[data-codex-voice-analyst-bar]")?.textContent).toContain(
      "codexVoiceAnalystNeedsInput",
    );
    expect(
      host.querySelector("[data-codex-voice-analyst-bar] button")?.getAttribute("aria-expanded"),
    ).toBe("false");
    expect(
      current.analyst?.permissions?.length,
      "new permission must reach collapsed summary",
    ).toBe(1);
  } finally {
    await act(async () => current.disconnect());
    await act(async () => root.unmount());
    host.remove();
  }
});
