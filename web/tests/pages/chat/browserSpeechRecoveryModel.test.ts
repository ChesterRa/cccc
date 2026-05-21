import { describe, expect, it } from "vitest";

import { shouldScheduleBrowserSpeechErrorRestart } from "../../../src/pages/chat/voice-secretary/browserSpeechRecoveryModel";

describe("browser speech recovery", () => {
  it("lets Web Speech network events recover passively", () => {
    expect(shouldScheduleBrowserSpeechErrorRestart("network")).toBe(false);
  });

  it("keeps active restart fallback for other recoverable events", () => {
    expect(shouldScheduleBrowserSpeechErrorRestart("no-speech")).toBe(true);
    expect(shouldScheduleBrowserSpeechErrorRestart("aborted")).toBe(true);
    expect(shouldScheduleBrowserSpeechErrorRestart("audio-capture")).toBe(true);
    expect(shouldScheduleBrowserSpeechErrorRestart("")).toBe(true);
  });
});
