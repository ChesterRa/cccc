import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CodexVoiceBrowserSession } from "./codexVoiceSession";

const mocks = vi.hoisted(() => ({ start: vi.fn(), stop: vi.fn(), ice: vi.fn() }));
vi.mock("../../services/api", () => ({
  startCodexVoiceCall: mocks.start,
  stopCodexVoiceCall: mocks.stop,
  prepareCodexVoiceNotificationOutput: vi.fn(),
}));
vi.mock("./codexVoiceMedia", async (original) => ({
  ...(await original<typeof import("./codexVoiceMedia")>()),
  waitForIceGathering: mocks.ice,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function fixture(setSinkId = vi.fn(async () => {})) {
  const track = { stop: vi.fn() };
  const stream = { getTracks: () => [track], getAudioTracks: () => [track] };
  const getUserMedia = vi.fn(async () => stream);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
  const wire = { close: vi.fn(), readyState: "connecting", bufferedAmount: 0 };
  const peer = {
    addTrack: vi.fn(),
    createDataChannel: () => wire,
    createOffer: vi.fn(async () => ({ type: "offer", sdp: "fixture-sdp" })),
    setLocalDescription: vi.fn(async () => {}),
    localDescription: { sdp: "fixture-sdp" },
    close: vi.fn(),
    ontrack: null as null | ((event: RTCTrackEvent) => void),
  };
  vi.stubGlobal(
    "RTCPeerConnection",
    class {
      constructor() {
        return peer;
      }
    },
  );
  const audio = { pause: vi.fn(), play: vi.fn(async () => {}), setSinkId, srcObject: null };
  const callbacks = {
    onPhase: vi.fn(),
    onCall: vi.fn(),
    onAnalyst: vi.fn(),
    onUserTranscript: vi.fn(),
    onAssistantTranscript: vi.fn(),
    onAnalystProgress: vi.fn(),
    onAnalystResult: vi.fn(),
    onPlaybackBlocked: vi.fn(),
    onError: vi.fn(),
  };
  const session = new CodexVoiceBrowserSession({
    audio: audio as unknown as HTMLAudioElement,
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    callbacks,
  });
  return { session, audio, callbacks, track, peer, getUserMedia };
}

beforeEach(() => {
  mocks.start.mockReset();
  mocks.stop.mockReset().mockResolvedValue({ ok: true, result: { stopped: true } });
  mocks.ice.mockReset().mockResolvedValue(undefined);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Voice startup cancellation", () => {
  it("does not request the microphone after stopping during speaker setup", async () => {
    const speaker = deferred<void>();
    const f = fixture(vi.fn(() => speaker.promise));
    const starting = f.session.start();
    await f.session.stop();
    speaker.resolve();
    await starting;
    expect(f.getUserMedia).not.toHaveBeenCalled();
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("does not start a provider call after stopping during ICE gathering", async () => {
    const ice = deferred<void>();
    mocks.ice.mockReturnValue(ice.promise);
    mocks.start.mockResolvedValue({ ok: false, error: { code: "unexpected_start" } });
    const f = fixture();
    const starting = f.session.start().catch(() => {});
    await vi.waitFor(() => expect(mocks.ice).toHaveBeenCalledOnce());
    await f.session.stop();
    ice.resolve();
    await starting;
    expect(mocks.start).not.toHaveBeenCalled();
    expect(f.callbacks.onPhase).toHaveBeenLastCalledWith("idle");
  });

  it("still reports a real startup failure and releases acquired media", async () => {
    mocks.ice.mockRejectedValue(new Error("ICE failed"));
    const f = fixture();
    await expect(f.session.start()).rejects.toThrow("ICE failed");
    expect(f.callbacks.onError).toHaveBeenCalledOnce();
    expect(f.callbacks.onPhase).toHaveBeenLastCalledWith("failed");
    expect(f.track.stop).toHaveBeenCalledOnce();
    expect(f.peer.close).toHaveBeenCalledOnce();
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("settles a cancelled startup quietly when an old asynchronous stage rejects", async () => {
    let rejectIce!: (error: Error) => void;
    mocks.ice.mockReturnValue(
      new Promise<void>((_resolve, reject) => {
        rejectIce = reject;
      }),
    );
    const f = fixture();
    const starting = f.session.start();
    await vi.waitFor(() => expect(mocks.ice).toHaveBeenCalledOnce());
    await f.session.stop();
    rejectIce(new Error("late ICE failure"));
    await expect(starting).resolves.toBeUndefined();
    expect(f.callbacks.onError).not.toHaveBeenCalled();
    expect(f.callbacks.onPhase).toHaveBeenLastCalledWith("idle");
  });

  it("does not publish a playback failure after the call was stopped", async () => {
    let rejectPlay!: (error: Error) => void;
    const f = fixture();
    f.audio.play.mockImplementation(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectPlay = reject;
        }),
    );
    const playing = f.session.resumeAudio();
    await f.session.stop();
    rejectPlay(new Error("late playback failure"));
    await playing;
    expect(f.callbacks.onPlaybackBlocked.mock.calls).toEqual([[false]]);
  });

  it("releases the exact generation returned after stop even if HTTP abort loses the race", async () => {
    const response = deferred<unknown>();
    mocks.start.mockReturnValue(response.promise);
    const f = fixture();
    const starting = f.session.start();
    await vi.waitFor(() => expect(mocks.start).toHaveBeenCalledOnce());
    await f.session.stop();
    response.resolve({ ok: true, result: { call: { generation: "cancelled-generation" } } });
    await starting;
    expect(mocks.stop).toHaveBeenCalledExactlyOnceWith("cancelled-generation");
    expect(f.callbacks.onCall.mock.calls).toEqual([[null]]);
  });

  it("ignores a queued remote track after the call has stopped", async () => {
    const ice = deferred<void>();
    mocks.ice.mockReturnValue(ice.promise);
    const f = fixture();
    const starting = f.session.start().catch(() => {});
    await vi.waitFor(() => expect(mocks.ice).toHaveBeenCalledOnce());
    const ontrack = f.peer.ontrack;
    await f.session.stop();
    ontrack?.({ streams: [{}] } as RTCTrackEvent);
    ice.resolve();
    await starting;
    expect(f.audio.srcObject).toBeNull();
    expect(f.audio.play).not.toHaveBeenCalled();
  });
});
