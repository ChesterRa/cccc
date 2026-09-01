import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CodexVoiceBrowserSession,
  eventStreamCloseCode,
  RealtimeTranscriptAccumulator,
  realtimeTranscriptUpdate,
  shouldForwardProviderEvent,
} from "./codexVoiceSession";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("Codex Voice realtime event model", () => {
  it("forwards only provider delegations to the Voice Analyst", () => {
    expect(shouldForwardProviderEvent({ type: "delegation.created" })).toBe(true);
    expect(shouldForwardProviderEvent({ type: "turn.done" })).toBe(false);
    expect(shouldForwardProviderEvent(null)).toBe(false);
  });

  it("preserves incremental whitespace and extracts final transcripts", () => {
    expect(
      realtimeTranscriptUpdate({ type: "input_transcript.added", item: { text: " hello " } }),
    ).toEqual({ role: "user", text: " hello ", final: false });
    expect(
      realtimeTranscriptUpdate({
        type: "turn.done",
        turn: { role: "assistant", transcript: "done" },
      }),
    ).toEqual({ role: "assistant", text: "done", final: true });
    expect(
      realtimeTranscriptUpdate({ type: "turn.done", turn: { role: "user", transcript: "" } }),
    ).toEqual({ role: "user", text: "", final: true });
    expect(realtimeTranscriptUpdate({ type: "turn.done", turn: { role: "tool" } })).toBeNull();
  });

  it("accumulates short streaming fragments until the authoritative final transcript", () => {
    const transcripts = new RealtimeTranscriptAccumulator();

    expect(transcripts.apply({ role: "user", text: "今", final: false })).toBe("今");
    expect(transcripts.apply({ role: "user", text: "天", final: false })).toBe("今天");
    expect(transcripts.apply({ role: "user", text: " 天气", final: false })).toBe("今天 天气");
    expect(transcripts.apply({ role: "user", text: "今天天气怎么样", final: true })).toBe(
      "今天天气怎么样",
    );
    expect(transcripts.apply({ role: "user", text: "下", final: false })).toBe("下");
  });

  it("keeps English word boundaries across streaming fragments", () => {
    const transcripts = new RealtimeTranscriptAccumulator();

    transcripts.apply({ role: "assistant", text: "Let me", final: false });
    expect(transcripts.apply({ role: "assistant", text: " check", final: false })).toBe(
      "Let me check",
    );
  });

  it("starts a fresh buffer when provider roles switch even if a final event is delayed", () => {
    const transcripts = new RealtimeTranscriptAccumulator();

    transcripts.apply({ role: "user", text: "first", final: false });
    transcripts.apply({ role: "assistant", text: "answer", final: false });
    expect(transcripts.apply({ role: "user", text: "second", final: false })).toBe("second");
  });

  it("preserves a specific server failure when the event stream closes", () => {
    expect(eventStreamCloseCode("analyst_disconnected")).toBe("analyst_disconnected");
    expect(eventStreamCloseCode("analyst_event_gap")).toBe("analyst_event_gap");
    expect(eventStreamCloseCode("Voice Analyst disconnected.")).toBe("event_stream_disconnected");
    expect(eventStreamCloseCode("")).toBe("event_stream_disconnected");
  });

  it("stops a microphone stream acquired after the session was stopped", async () => {
    let resolveCapture: ((stream: MediaStream) => void) | undefined;
    const pendingCapture = new Promise<MediaStream>((resolve) => {
      resolveCapture = resolve;
    });
    const stopTrack = vi.fn();
    const stream = {
      getTracks: () => [{ stop: stopTrack }],
      getAudioTracks: () => [{ stop: stopTrack }],
    } as unknown as MediaStream;
    const getUserMedia = vi.fn(() => pendingCapture);
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    vi.stubGlobal("RTCPeerConnection", class {});
    const audio = {
      pause: vi.fn(),
      play: vi.fn(async () => undefined),
      srcObject: null,
    } as unknown as HTMLAudioElement;
    const session = new CodexVoiceBrowserSession({
      groupId: "g_voice",
      audio,
      preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
      callbacks: {
        onPhase: vi.fn(),
        onCall: vi.fn(),
        onAnalyst: vi.fn(),
        onUserTranscript: vi.fn(),
        onAssistantTranscript: vi.fn(),
        onAnalystProgress: vi.fn(),
        onAnalystResult: vi.fn(),
        onPlaybackBlocked: vi.fn(),
        onError: vi.fn(),
      },
    });

    const starting = session.start();
    await vi.waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    await session.stop();
    resolveCapture?.(stream);
    await starting;

    expect(stopTrack).toHaveBeenCalledOnce();
  });
});
