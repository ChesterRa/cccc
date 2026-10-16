import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { CodexVoiceBrowserSession } from "./codexVoiceSession";
import type { CodexVoiceServerMessage } from "./codexVoiceEventSocket";

const fixture = vi.hoisted(() => ({
  start: vi.fn(),
  stop: vi.fn(),
  capture: vi.fn(),
  output: vi.fn(),
  controlClosed: vi.fn(),
  audioClosed: vi.fn(),
  server: null as ((message: CodexVoiceServerMessage) => void) | null,
  sent: [] as unknown[],
  phases: [] as string[],
}));
vi.mock("../../services/api", () => ({
  startCodexVoiceCall: fixture.start,
  stopCodexVoiceCall: fixture.stop,
  prepareCodexVoiceNotificationOutput: vi.fn(),
}));
vi.mock("./codexVoiceMedia", () => ({
  captureMicrophone: fixture.capture,
  applyOutputDevice: fixture.output,
  createClientSessionId: () => "grok-client",
}));
vi.mock("./codexVoiceEventSocket", () => ({
  CodexVoiceEventSocket: class {
    constructor(_call: unknown, onMessage: typeof fixture.server) {
      fixture.server = onMessage;
    }
    async connect() {
      fixture.server!({
        type: "provider_command",
        message: {
          type: "session.context.append",
          channel: "speakable",
          content: [{ type: "input_text", text: "Greet briefly" }],
        },
      });
    }
    send(message: unknown) {
      fixture.sent.push(message);
      return true;
    }
    close() {
      fixture.controlClosed();
    }
  },
}));
vi.mock("./grokVoiceAudio", () => ({
  GrokVoiceAudio: class {
    async startCapture() {}
    append() {}
    interrupt() {}
    hasResponse() {
      return false;
    }
    isPlaying() {
      return false;
    }
    async resume() {
      return true;
    }
    close() {
      fixture.audioClosed();
    }
  },
}));
class Socket {
  static OPEN = 1;
  static latest: Socket;
  readyState = 1;
  bufferedAmount = 0;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose = null;
  onerror = null;
  frames: unknown[] = [];
  constructor() {
    Socket.latest = this;
    queueMicrotask(() => this.emit({ type: "session.created" }));
  }
  emit(event: unknown) {
    this.onmessage?.({ data: JSON.stringify(event) });
  }
  send(text: string) {
    const event = JSON.parse(text);
    this.frames.push(event);
    if (event.type === "session.update")
      queueMicrotask(() => this.emit({ type: "session.updated" }));
  }
  close() {
    this.readyState = 3;
  }
}
const result = {
  call: {
    provider: "grok",
    generation: "grok-call",
    analyst_generation: "retained-analyst",
    connected: false,
    voice: "ara",
  },
  analyst: null,
  answer_sdp: "",
  provider_connection: {
    transport: "websocket_pcm",
    url: "wss://fixture.invalid",
    client_secret: "fixture-short-lived",
    expires_at: 9_999_999_999,
    session: { voice: "ara" },
  },
};
let session: CodexVoiceBrowserSession;
const track = { enabled: true, stop: vi.fn() };
beforeEach(() => {
  vi.clearAllMocks();
  fixture.sent = [];
  fixture.phases = [];
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn() } });
  vi.stubGlobal("WebSocket", Socket);
  vi.stubGlobal("RTCPeerConnection", undefined);
  fixture.start.mockResolvedValue({ ok: true, result });
  fixture.stop.mockResolvedValue({ ok: true, result: {} });
  fixture.capture.mockResolvedValue({ getTracks: () => [track], getAudioTracks: () => [track] });
  session = new CodexVoiceBrowserSession({
    audio: { pause: vi.fn() } as unknown as HTMLAudioElement,
    preferences: {
      provider: "grok",
      voice: "ara",
      inputDeviceId: "shared-mic",
      outputDeviceId: "shared-speaker",
    },
    callbacks: {
      onPhase: (phase) => fixture.phases.push(phase),
      onCall: vi.fn(),
      onAnalyst: vi.fn(),
      onError: vi.fn(),
      onUserTranscript: vi.fn(),
      onAssistantTranscript: vi.fn(),
      onAnalystProgress: vi.fn(),
      onAnalystResult: vi.fn(),
      onPlaybackBlocked: vi.fn(),
    },
  });
});
afterEach(async () => {
  await session.stop();
  vi.unstubAllGlobals();
});

it("uses the existing call owner, shared devices and control socket without requiring WebRTC", async () => {
  await session.start();
  expect(fixture.capture).toHaveBeenCalledWith("shared-mic");
  expect(fixture.output).toHaveBeenCalledWith(expect.anything(), "shared-speaker");
  expect(fixture.start).toHaveBeenCalledWith(
    expect.objectContaining({
      provider: "grok",
      voice: "ara",
      offerSdp: undefined,
      clientSessionId: "grok-client",
    }),
  );
  expect(fixture.phases.at(-1)).toBe("listening");
  expect(session.setMicrophoneMuted(true)).toBe(true);
  expect(track.enabled).toBe(false);
  Socket.latest.emit({
    type: "conversation.item.input_audio_transcription.updated",
    item_id: "u1",
    transcript: "Check Group A kilo",
  });
  Socket.latest.emit({
    type: "conversation.item.input_audio_transcription.completed",
    item_id: "u1",
    transcript: "Check Group A Kiro. Only diagnose; no configuration changes or restart.",
  });
  expect(fixture.sent).not.toContainEqual(expect.objectContaining({ type: "provider_event" }));
  Socket.latest.emit({
    type: "response.function_call_arguments.done",
    call_id: "function-1",
    name: "cccc_investigate",
    arguments: '{"request":"Inspect"}',
  });
  expect(fixture.sent).toContainEqual(
    expect.objectContaining({
      type: "provider_event",
      event: expect.objectContaining({ call_id: "function-1" }),
      context: {
        messages: [
          {
            role: "user",
            text: "Check Group A Kiro. Only diagnose; no configuration changes or restart.",
            incomplete: false,
          },
        ],
        truncated: false,
      },
    }),
  );
  fixture.server!({
    type: "provider_command",
    message: { type: "cccc.delegation.receipt", call_id: "function-1", accepted: true },
  });
  expect(Socket.latest.frames).toContainEqual({
    type: "conversation.item.create",
    item: { type: "function_call_output", call_id: "function-1", output: '{"status":"received"}' },
  });
  await session.stop();
  expect(track.stop).toHaveBeenCalled();
  expect(fixture.audioClosed).toHaveBeenCalled();
  expect(fixture.controlClosed).toHaveBeenCalled();
  expect(fixture.stop).toHaveBeenCalledWith("grok-call");
  expect(Socket.latest.readyState).toBe(3);
});

it("releases a late accepted generation when Stop wins the startup request", async () => {
  let finish!: (value: unknown) => void;
  fixture.start.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const starting = session.start();
  await vi.waitFor(() => expect(fixture.start).toHaveBeenCalled());
  await session.stop();
  finish({ ok: true, result });
  await starting;
  expect(fixture.stop).toHaveBeenCalledExactlyOnceWith("grok-call");
  expect(track.stop).toHaveBeenCalled();
  expect(fixture.phases.at(-1)).toBe("idle");
});
