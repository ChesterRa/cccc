import { describe, expect, it } from "vitest";

import { voiceFinalAsrProgressLabel } from "../../../src/pages/chat/voice-secretary/voiceFinalAsrProgress";

const t = (key: string, opts?: Record<string, unknown>) => {
  let text = String(opts?.defaultValue || key);
  Object.entries(opts || {}).forEach(([name, value]) => {
    text = text.replace(`{{${name}}}`, String(value));
  });
  return text;
};

describe("voice final ASR progress labels", () => {
  it("formats segment progress", () => {
    expect(voiceFinalAsrProgressLabel({
      type: "final_asr_progress",
      stage: "transcribing",
      segment_index: 2,
      segment_count: 5,
    }, t)).toBe("Final ASR 2/5...");
  });

  it("formats final ASR fallback status", () => {
    expect(voiceFinalAsrProgressLabel({ type: "final_asr_failed" }, t)).toBe("Final ASR failed; keeping live transcript.");
  });
});
