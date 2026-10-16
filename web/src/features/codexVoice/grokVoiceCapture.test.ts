import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, it } from "vitest";

it.each([44_100, 48_000, 96_000])(
  "captures one second at %i Hz as fifty PCM16/24k frames",
  (rate) => {
    const frames: ArrayBuffer[] = [];
    let Processor: new () => { process(inputs: Float32Array[][]): boolean };
    runInNewContext(
      readFileSync(new URL("./grokVoiceCapture.worklet.js", import.meta.url), "utf8"),
      {
        sampleRate: rate,
        Int16Array,
        AudioWorkletProcessor: class {
          port = { postMessage: (buffer: ArrayBuffer) => frames.push(buffer) };
        },
        registerProcessor: (_name: string, value: typeof Processor) => {
          Processor = value;
        },
      },
    );
    const processor = new Processor!();
    for (let start = 0; start < rate; start += 128) {
      const samples = Float32Array.from(
        { length: Math.min(128, rate - start) },
        (_, i) => Math.sin((2 * Math.PI * 440 * (start + i)) / rate) * 0.5,
      );
      expect(processor.process([[samples]])).toBe(true);
    }
    expect(frames).toHaveLength(50);
    expect(frames.every((frame) => frame.byteLength === 960)).toBe(true);
    const samples = frames.flatMap((frame) => [...new Int16Array(frame)]);
    const zeroCrossings = samples
      .slice(1)
      .filter((sample, i) => sample >= 0 && samples[i] < 0).length;
    expect(zeroCrossings).toBeGreaterThanOrEqual(439);
    expect(zeroCrossings).toBeLessThanOrEqual(440);
    const rms = Math.sqrt(
      samples.reduce((sum, value) => sum + (value / 32768) ** 2, 0) / samples.length,
    );
    expect(rms).toBeGreaterThan(0.34);
    expect(rms).toBeLessThan(0.36);
  },
);
