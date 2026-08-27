import { describe, expect, it } from "vite-plus/test";

import { Pcm16Resampler } from "./voicePcmResampler";

function pcm16(bytes: Uint8Array): number[] {
  return Array.from(new Int16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 2));
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((total, part) => total + part.byteLength, 0));
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.byteLength;
  }
  return output;
}

describe("Pcm16Resampler", () => {
  it("preserves the source amplitude instead of applying a second capture gain", () => {
    const output = pcm16(new Pcm16Resampler(16_000).push(new Float32Array([0.5, -0.5])));

    expect(output).toEqual([16_383, -16_384]);
  });

  it("averages downsampled input without clipping normal speech peaks", () => {
    const output = pcm16(new Pcm16Resampler(48_000).push(new Float32Array([0.75, 0.75, 0.75])));

    expect(output).toEqual([24_575]);
  });

  it("produces the same 44.1 kHz output regardless of browser chunk boundaries", () => {
    const input = Float32Array.from(
      { length: 4_410 },
      (_, index) => Math.sin((2 * Math.PI * 440 * index) / 44_100) * 0.5,
    );
    const complete = new Pcm16Resampler(44_100).push(input);
    const chunkedResampler = new Pcm16Resampler(44_100);
    const chunks: Uint8Array[] = [];
    for (let offset = 0; offset < input.length; offset += 4_096) {
      chunks.push(chunkedResampler.push(input.slice(offset, offset + 4_096)));
    }

    expect(complete.byteLength / 2).toBe(1_600);
    expect(concatBytes(chunks)).toEqual(complete);
  });
});
