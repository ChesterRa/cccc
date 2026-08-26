import { describe, expect, it } from "vite-plus/test";

import { createVoiceRequestDispatchGate } from "../../../src/pages/chat/voice-secretary/voiceRequestDispatchGate";

describe("voiceRequestDispatchGate", () => {
  it("rejects a concurrent duplicate for the same target", () => {
    const gate = createVoiceRequestDispatchGate();
    expect(gate.tryAcquire("panel:g1")).toBe(true);
    expect(gate.tryAcquire("panel:g1")).toBe(false);
    expect(gate.isActive("panel:g1")).toBe(true);
  });

  it("releases after success, failure, or cancellation cleanup", () => {
    const gate = createVoiceRequestDispatchGate();
    expect(gate.tryAcquire("panel:g1")).toBe(true);
    gate.release("panel:g1");
    expect(gate.isActive("panel:g1")).toBe(false);
    expect(gate.tryAcquire("panel:g1")).toBe(true);
  });

  it("does not serialize unrelated targets", () => {
    const gate = createVoiceRequestDispatchGate();
    expect(gate.tryAcquire("panel:g1")).toBe(true);
    expect(gate.tryAcquire("panel:g2")).toBe(true);
  });
});
