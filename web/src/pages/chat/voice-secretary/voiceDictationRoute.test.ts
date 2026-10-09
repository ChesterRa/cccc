import { describe, expect, it } from "vite-plus/test";
import { voiceCaptureDispatchTarget, voiceCaptureTransportMode } from "./voiceDictationRoute";

describe("voice dictation routing", () => {
  it("defaults Prompt to dictation without a model task", () => {
    const target = voiceCaptureDispatchTarget({ captureMode: "prompt" });
    expect(target).toBe("composer");
    expect(voiceCaptureTransportMode(target)).toBe("prompt");
  });
  it("only requests refinement when automatic polishing is enabled", () => {
    expect(voiceCaptureDispatchTarget({ captureMode: "prompt", promptAutoRefine: true })).toBe(
      "prompt",
    );
    expect(voiceCaptureDispatchTarget({ captureMode: "prompt", promptAutoRefine: false })).toBe(
      "composer",
    );
  });
  it.each(["document", "instruction"] as const)(
    "keeps %s model-assisted capture on its explicit route",
    (captureMode) => {
      const target = voiceCaptureDispatchTarget({ captureMode });
      expect(target).toBe(captureMode);
      expect(voiceCaptureTransportMode(target)).toBe(captureMode);
      expect(voiceCaptureDispatchTarget({ captureMode, promptAutoRefine: true })).toBe(captureMode);
    },
  );
});
