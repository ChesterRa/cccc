// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { getUserMediaWithTimeout, VoiceAudioCaptureTimeoutError } from "./voiceAudioCapture";

describe("getUserMediaWithTimeout", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("returns a microphone stream that resolves before the deadline", async () => {
    const stream = { getTracks: vi.fn(() => []) } as unknown as MediaStream;
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn(async () => stream) } });
    await expect(getUserMediaWithTimeout({ audio: true }, 50)).resolves.toBe(stream);
  });

  it("times out a pending permission request and stops a late stream", async () => {
    vi.useFakeTimers();
    let resolveCapture: (stream: MediaStream) => void = () => undefined;
    const capture = new Promise<MediaStream>((resolve) => {
      resolveCapture = resolve;
    });
    const stop = vi.fn();
    const stream = { getTracks: () => [{ stop }] } as unknown as MediaStream;
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: vi.fn(() => capture) } });

    const pending = getUserMediaWithTimeout({ audio: true }, 20);
    const rejection = expect(pending).rejects.toBeInstanceOf(VoiceAudioCaptureTimeoutError);
    await vi.advanceTimersByTimeAsync(20);
    await rejection;
    resolveCapture(stream);
    await Promise.resolve();
    expect(stop).toHaveBeenCalledOnce();
  });
});
