import { describe, expect, it } from "vite-plus/test";

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

import {
  BrowserSpeechRecoveryBudget,
  isQuietBrowserSpeechError,
} from "../../../src/pages/chat/voice-secretary/browserSpeechRecoveryModel";

describe("browser speech restart budget", () => {
  it.each(["network", "audio-capture", "start-failed"])(
    "stops repeated %s failures instead of silently looping",
    (error) => {
      const budget = new BrowserSpeechRecoveryBudget();
      for (let attempt = 1; attempt <= 8; attempt++) {
        budget.beginCycle();
        budget.recordError(error);
        budget.recordError(error); // Duplicate callbacks are still one failed attempt.
        expect(budget.endCycle(100)).toBe(attempt === 8);
        expect(budget.failures).toBe(attempt);
      }
      expect(isQuietBrowserSpeechError(error)).toBe(false);
    },
  );

  it("bounds rapid empty end cycles even without an error event", () => {
    const budget = new BrowserSpeechRecoveryBudget();
    for (let attempt = 1; attempt <= 8; attempt++) {
      budget.beginCycle();
      expect(budget.endCycle(100)).toBe(attempt === 8);
    }
  });

  it("preserves long quiet sessions and successful continuous dictation", () => {
    const budget = new BrowserSpeechRecoveryBudget();
    for (let cycle = 0; cycle < 20; cycle++) {
      budget.beginCycle();
      budget.recordError("no-speech");
      expect(budget.endCycle(30_000)).toBe(false);
      budget.beginCycle();
      budget.recordResult();
      expect(budget.endCycle(100)).toBe(false);
    }
    expect(budget.failures).toBe(0);
  });

  it("successful results and a new user recording reset previous failures", () => {
    const budget = new BrowserSpeechRecoveryBudget(2);
    budget.beginCycle();
    budget.recordError("network");
    budget.recordResult();
    expect(budget.failures).toBe(0);
    budget.beginCycle();
    budget.recordError("network");
    expect(budget.exhausted).toBe(false);
    budget.beginCycle();
    budget.recordError("network");
    expect(budget.exhausted).toBe(true);
    budget.reset();
    expect(budget.exhausted).toBe(false);
    expect(budget.failures).toBe(0);
  });
});
