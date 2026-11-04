import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, it } from "vite-plus/test";

it.each([44_100, 48_000, 96_000])(
  "preserves input order and flushes the incomplete block at %i Hz",
  (rate) => {
    const messages: Array<{ type: string; buffer?: ArrayBuffer }> = [];
    let Processor!: new () => {
      process(inputs: Float32Array[][]): boolean;
      port: { onmessage: (event: { data: { type: string } }) => void };
    };
    runInNewContext(
      readFileSync(new URL("./voicePcmCapture.worklet.js", import.meta.url), "utf8"),
      {
        sampleRate: rate,
        Float32Array,
        AudioWorkletProcessor: class {
          port = {
            postMessage: (message: { type: string; buffer?: ArrayBuffer }) =>
              messages.push(
                message.buffer
                  ? structuredClone(message, { transfer: [message.buffer] })
                  : structuredClone(message),
              ),
          };
        },
        registerProcessor: (_name: string, value: typeof Processor) => {
          Processor = value;
        },
      },
    );
    const processor = new Processor();
    const input = Float32Array.from({ length: rate + 73 }, (_, i) => (i % 97) / 100);
    for (let start = 0; start < input.length; start += 173) {
      expect(processor.process([[input.subarray(start, start + 173)]])).toBe(true);
    }
    processor.port.onmessage({ data: { type: "flush" } });
    expect(messages.at(-1)?.type).toBe("flushed");
    const output = messages
      .filter((message) => message.type === "samples")
      .flatMap((message) => [...new Float32Array(message.buffer!)]);
    expect(output).toEqual([...input]);
    expect(processor.process([[new Float32Array([0.5])]])).toBe(false);
    expect(messages.at(-1)?.type).toBe("flushed");
  },
);
