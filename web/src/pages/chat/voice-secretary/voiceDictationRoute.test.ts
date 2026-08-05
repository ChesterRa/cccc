import { describe, expect, it } from "vite-plus/test";
import { voiceCaptureDispatchTarget, voiceCaptureTransportMode } from "./voiceDictationRoute";

describe("voice dictation routing", () => {
  it.each(["document", "instruction", "prompt"] as const)(
    "routes disabled-assistant %s capture directly to the composer",
    (captureMode) => {
      const target = voiceCaptureDispatchTarget({ assistantEnabled: false, captureMode });
      expect(target).toBe("composer");
      expect(voiceCaptureTransportMode(target)).toBe("prompt");
    },
  );

  it.each(["document", "instruction", "prompt"] as const)(
    "preserves enabled-assistant %s routing",
    (captureMode) => {
      const target = voiceCaptureDispatchTarget({ assistantEnabled: true, captureMode });
      expect(target).toBe(captureMode);
      expect(voiceCaptureTransportMode(target)).toBe(captureMode);
    },
  );
});
