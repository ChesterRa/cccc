import { describe, expect, it } from "vitest";
import {
  CODEX_REALTIME_VOICES,
  DEFAULT_CODEX_REALTIME_VOICE,
  normalizeCodexRealtimeVoice,
  normalizeCodexVoicePreferences,
} from "./codexVoicePreferences";

describe("Codex Voice preferences", () => {
  it("matches the compatibility-pinned v3 voice allowlist", () => {
    expect(CODEX_REALTIME_VOICES).toEqual([
      "juniper",
      "maple",
      "spruce",
      "ember",
      "vale",
      "breeze",
      "arbor",
      "sol",
      "cove",
    ]);
    expect(DEFAULT_CODEX_REALTIME_VOICE).toBe("cove");
  });

  it("normalizes unknown values and bounds browser device ids", () => {
    expect(normalizeCodexRealtimeVoice(" Cove ")).toBe("cove");
    expect(normalizeCodexRealtimeVoice("unknown")).toBe("cove");
    expect(
      normalizeCodexVoicePreferences({
        voice: "maple",
        inputDeviceId: " microphone ",
        outputDeviceId: "x".repeat(513),
      }),
    ).toEqual({ voice: "maple", inputDeviceId: "microphone", outputDeviceId: "" });
  });
});
