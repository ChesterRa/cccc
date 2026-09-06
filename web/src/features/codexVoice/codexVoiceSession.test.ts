import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CodexVoiceBrowserSession,
  eventStreamCloseCode,
  RealtimeTranscriptAccumulator,
  realtimeTranscriptUpdate,
  shouldForwardProviderEvent,
} from "./codexVoiceSession";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Codex Voice realtime event model", () => {
  it("ignores provider data decoded after stop and late Analyst messages", async () => {
    const onPhase = vi.fn();
    const onAssistantTranscript = vi.fn();
    const onAnalystProgress = vi.fn();
    const session = new CodexVoiceBrowserSession({
      audio: { pause: vi.fn() } as unknown as HTMLAudioElement,
      preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
      callbacks: {
        onPhase,
        onCall: vi.fn(),
        onAnalyst: vi.fn(),
        onUserTranscript: vi.fn(),
        onAssistantTranscript,
        onAnalystProgress,
        onAnalystResult: vi.fn(),
        onPlaybackBlocked: vi.fn(),
        onError: vi.fn(),
      },
    });
    let finish: ((value: string) => void) | undefined;
    const blob = new Blob();
    vi.spyOn(blob, "text").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const transport = session as unknown as {
      handleProviderMessage(value: unknown): Promise<void>;
      handleServerMessage(value: Record<string, unknown>): void;
    };
    const pending = transport.handleProviderMessage(blob);
    await session.stop();
    onPhase.mockClear();
    finish?.(
      JSON.stringify({ type: "turn.done", turn: { role: "assistant", transcript: "Late." } }),
    );
    await pending;
    transport.handleServerMessage({ type: "analyst_progress", text: "Late progress." });
    expect(onPhase).not.toHaveBeenCalled();
    expect(onAssistantTranscript).not.toHaveBeenCalled();
    expect(onAnalystProgress).not.toHaveBeenCalled();
  });

  it("reports all positively unsent IDs in bounded frames before overflow teardown", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const frames: Record<string, unknown>[] = [];
    const onError = vi.fn();
    const session = new CodexVoiceBrowserSession({
      audio: { pause: vi.fn() } as unknown as HTMLAudioElement,
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
        onError,
      },
    });
    const transport = session as unknown as {
      eventSocket: { send(value: Record<string, unknown>): boolean; close(): void };
      handleServerMessage(value: Record<string, unknown>): void;
      handleProviderMessage(value: string): Promise<void>;
    };
    transport.eventSocket = {
      send: (value) => {
        frames.push(value);
        return true;
      },
      close: () => frames.push({ type: "socket_closed" }),
    };
    await transport.handleProviderMessage(
      JSON.stringify({ type: "turn.created", turn: { id: "u1", role: "user" } }),
    );
    const ids = Array.from({ length: 1025 }, (_, i) => `result-${i}:`.padEnd(256, "x"));
    for (const resultId of ids)
      transport.handleServerMessage({
        type: "provider_command",
        result_id: resultId,
        message: {
          type: "session.context.append",
          channel: "speakable",
          content: [{ type: "input_text", text: "Queued" }],
        },
      });
    const reports = frames.filter((frame) => frame.type === "notification_output_not_submitted");
    expect(reports.flatMap((frame) => frame.result_ids)).toEqual(ids);
    for (const report of reports) {
      expect((report.result_ids as string[]).length).toBeLessThanOrEqual(1024);
      expect(new TextEncoder().encode(JSON.stringify(report)).length).toBeLessThanOrEqual(
        128 * 1024,
      );
      expect(frames.indexOf(report)).toBeLessThan(
        frames.findIndex((frame) => frame.type === "stop"),
      );
    }
    expect(frames.at(-1)?.type).toBe("socket_closed");
    expect(onError).toHaveBeenCalledExactlyOnceWith("provider_command_overflow", undefined);
    await session.stop();
  });

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

  it("keeps turn order and updates delayed final text without duplicating conversation entries", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    transcripts.observeTurn({ type: "turn.created", turn: { id: "u1", role: "user" } });
    transcripts.apply({ role: "user", text: "question", final: false });
    transcripts.observeTurn({ type: "turn.created", turn: { id: "a1", role: "assistant" } });
    transcripts.apply({ role: "assistant", text: "answer", final: false });
    transcripts.observeTurn({ type: "turn.created", turn: { id: "u2", role: "user" } });
    transcripts.apply({ role: "user", text: "follow", final: false });
    transcripts.apply({ role: "user", turnId: "u1", text: "Question?", final: true });
    transcripts.apply({ role: "user", text: " up", final: false });
    transcripts.apply({ role: "assistant", turnId: "a1", text: "Answer.", final: true });
    transcripts.apply({ role: "assistant", turnId: "a1", text: "Answer.", final: true });
    expect(transcripts.history()).toEqual([
      { id: "u1", role: "user", text: "Question?", final: true },
      { id: "a1", role: "assistant", text: "Answer.", final: true },
      { id: "u2", role: "user", text: "follow up", final: false },
    ]);
  });

  it("bounds conversation history and individual entries, omitting empty turns", () => {
    const transcripts = new RealtimeTranscriptAccumulator();
    for (let n = 0; n < 45; n++) {
      transcripts.apply({
        role: "assistant",
        turnId: String(n),
        text: "x".repeat(5000),
        final: true,
      });
    }
    expect(transcripts.history()).toHaveLength(40);
    expect(transcripts.history()[0].id).toBe("5");
    expect(transcripts.history()[0].text).toHaveLength(4000);
    transcripts.apply({ role: "assistant", turnId: "44", text: "", final: true });
    expect(transcripts.history()).toHaveLength(39);
  });

  it("surfaces paused notifications without interrupting audio or letting Analyst events overwrite voice state", async () => {
    const onPhase = vi.fn();
    const onNotificationPaused = vi.fn();
    const onError = vi.fn();
    const session = new CodexVoiceBrowserSession({
      audio: {} as HTMLAudioElement,
      preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
      callbacks: {
        onPhase,
        onNotificationPaused,
        onError,
        onCall: vi.fn(),
        onAnalyst: vi.fn(),
        onUserTranscript: vi.fn(),
        onAssistantTranscript: vi.fn(),
        onAnalystProgress: vi.fn(),
        onAnalystResult: vi.fn(),
        onPlaybackBlocked: vi.fn(),
      },
    });
    const transport = session as unknown as {
      handleServerMessage(value: Record<string, unknown>): void;
      handleProviderMessage(value: string): Promise<void>;
    };
    await transport.handleProviderMessage(
      JSON.stringify({ type: "output_transcript.added", item: { text: "Still speaking" } }),
    );
    expect(onPhase).toHaveBeenLastCalledWith("speaking");
    onPhase.mockClear();
    for (const type of ["analyst_working", "analyst_terminal", "analyst_cancelling"]) {
      transport.handleServerMessage({ type });
    }
    transport.handleServerMessage({ type: "notification_status", paused: true });
    expect(onNotificationPaused).toHaveBeenCalledExactlyOnceWith(true);
    expect(onPhase).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
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
