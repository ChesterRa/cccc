import { describe, expect, it } from "vite-plus/test";
import {
  drainVoicePcmQueue,
  flushVoicePcmQueue,
  queueVoicePcmFrame,
  type VoicePcmSocket,
} from "./voicePcmBackpressure";

function socket(bufferedAmount = 0): VoicePcmSocket & { sent: Uint8Array[] } {
  return {
    readyState: 1,
    bufferedAmount,
    sent: [],
    send(frame) {
      this.sent.push(frame);
      this.bufferedAmount += frame.byteLength;
    },
  };
}

describe("voice PCM backpressure", () => {
  it("sends immediately while the websocket is below its high-water mark", () => {
    const active = socket();
    const pending: Uint8Array[] = [];

    const result = queueVoicePcmFrame(active, pending, new Uint8Array([1, 2]));

    expect(result.droppedBytes).toBe(0);
    expect(active.sent).toHaveLength(1);
    expect(pending).toHaveLength(0);
  });

  it("bounds queued audio and drains it in order after pressure clears", () => {
    const active = socket(256 * 1024);
    const pending: Uint8Array[] = [];
    const frame = new Uint8Array(64 * 1024);
    let droppedBytes = 0;

    for (let index = 0; index < 6; index += 1) {
      droppedBytes += queueVoicePcmFrame(active, pending, frame).droppedBytes;
    }
    expect(droppedBytes).toBe(2 * 64 * 1024);
    expect(pending).toHaveLength(4);

    active.bufferedAmount = 0;
    flushVoicePcmQueue(active, pending);
    expect(active.sent).toHaveLength(4);
    expect(pending).toHaveLength(0);
  });

  it("flushes the bounded tail before the stop control message", () => {
    const active = socket(256 * 1024);
    const pending = [new Uint8Array([1]), new Uint8Array([2])];

    drainVoicePcmQueue(active, pending);

    expect(active.sent.map((frame) => frame[0])).toEqual([1, 2]);
    expect(pending).toHaveLength(0);
  });

  it("rejects a single frame larger than the bounded pending queue", () => {
    const pending: Uint8Array[] = [];
    const oversized = new Uint8Array(256 * 1024 + 1);

    const result = queueVoicePcmFrame(null, pending, oversized);

    expect(result.droppedBytes).toBe(oversized.byteLength);
    expect(pending).toHaveLength(0);
  });
});
