import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { GrokVoiceAudio } from "./grokVoiceAudio";

type Source = {
  buffer: { duration: number } | null;
  start: ReturnType<typeof vi.fn>;
  stop: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
  onended: (() => void) | null;
};
let now = 0;
let nodes: Source[];
let audio: GrokVoiceAudio;
let speaking: ReturnType<typeof vi.fn>;
let drained: ReturnType<typeof vi.fn>;
const frame = btoa("\0".repeat(960)); // 20ms of PCM16 / 24kHz silence.

beforeEach(() => {
  now = 0;
  nodes = [];
  vi.stubGlobal(
    "AudioContext",
    class {
      get currentTime() {
        return now;
      }
      audioWorklet = { addModule: async () => {} };
      createMediaStreamDestination() {
        return { stream: { getTracks: () => [] } };
      }
      createBuffer(_channels: number, length: number, rate: number) {
        const samples = new Float32Array(length);
        return { duration: length / rate, getChannelData: () => samples };
      }
      createBufferSource() {
        const node = {
          buffer: null,
          start: vi.fn(),
          stop: vi.fn(),
          connect: vi.fn(),
          disconnect: vi.fn(),
          onended: null,
        };
        nodes.push(node);
        return node;
      }
      async resume() {}
      async close() {}
    },
  );
  vi.stubGlobal("AudioWorkletNode", class {});
  speaking = vi.fn();
  drained = vi.fn();
  audio = new GrokVoiceAudio(
    { play: async () => {}, pause: () => {} } as unknown as HTMLAudioElement,
    speaking,
    drained,
    vi.fn(),
  );
});

afterEach(() => {
  audio.close();
  vi.unstubAllGlobals();
});

const start = (node: Source) => Number(node.start.mock.calls[0][0]);

it("absorbs a small arrival delay without gaps between PCM frames", () => {
  for (const arrival of [0, 0.015, 0.045, 0.06]) {
    now = arrival;
    audio.append(frame, "answer");
  }
  expect(start(nodes[0])).toBeGreaterThan(0);
  expect(start(nodes[0])).toBeLessThanOrEqual(0.1);
  for (let index = 1; index < nodes.length; index++) {
    expect(start(nodes[index])).toBeCloseTo(start(nodes[index - 1]) + 0.02);
    expect(nodes[index].buffer?.duration).toBe(0.02);
  }
});

it("restarts with bounded buffering after a longer gap", () => {
  audio.append(frame, "answer");
  now = 0.5;
  audio.append(frame, "answer");
  expect(start(nodes[1])).toBeGreaterThan(now);
  expect(start(nodes[1])).toBeLessThanOrEqual(now + 0.1);
  expect(nodes[1].buffer?.duration).toBe(0.02);
});

it("keeps response playback consecutive and drains each response independently", () => {
  audio.append(frame, "first");
  now = 0.005;
  audio.append(frame, "second");
  expect(start(nodes[1])).toBeCloseTo(start(nodes[0]) + 0.02);
  nodes[0].onended?.();
  expect(drained).toHaveBeenLastCalledWith("first");
  expect(audio.isPlaying()).toBe(true);
  expect(audio.hasResponse("second")).toBe(true);
  nodes[1].onended?.();
  expect(drained).toHaveBeenLastCalledWith("second");
  expect(speaking).toHaveBeenLastCalledWith(false);
  expect(audio.isPlaying()).toBe(false);
});

it("discards scheduled audio on interruption and starts the next reply promptly", () => {
  for (let index = 0; index < 25; index++) audio.append(frame, "old");
  const oldNodes = [...nodes];
  audio.interrupt();
  expect(audio.isPlaying()).toBe(false);
  expect(oldNodes.every((node) => node.stop.mock.calls.length === 1 && !node.onended)).toBe(true);
  expect(drained).not.toHaveBeenCalled();
  now = 0.1;
  audio.append(frame, "new");
  expect(start(nodes.at(-1)!)).toBeLessThanOrEqual(now + 0.1);
  expect(audio.hasResponse("old")).toBe(false);
  expect(audio.hasResponse("new")).toBe(true);
});
