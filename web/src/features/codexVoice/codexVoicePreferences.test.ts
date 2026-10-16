import { describe, expect, it } from "vitest";
import {
  DEFAULT_CODEX_REALTIME_VOICE,
  codexVoicePrimaryLanguage,
  normalizeCodexRealtimeVoice,
  normalizeCodexVoicePreferences,
} from "./codexVoicePreferences";

describe("Codex Voice preferences", () => {
  it("keeps Cove as the bootstrap default while the server catalog loads", () => {
    expect(DEFAULT_CODEX_REALTIME_VOICE).toBe("cove");
  });

  it("labels documented language profiles without guessing unknown voice languages", () => {
    expect(codexVoicePrimaryLanguage(" Himari ")).toBe("ja");
    expect(codexVoicePrimaryLanguage("rindo")).toBe("ja");
    expect(codexVoicePrimaryLanguage("hanul")).toBe("ko");
    expect(codexVoicePrimaryLanguage("miko")).toBe("de");
    expect(codexVoicePrimaryLanguage("rio")).toBe("pt-BR");
    expect(codexVoicePrimaryLanguage("tinta")).toBe("es-MX");
    for (const voice of ["cove", "porto", "selva", "unknown"]) {
      expect(codexVoicePrimaryLanguage(voice)).toBeUndefined();
    }
  });

  it("preserves server-owned voice identifiers and bounds browser device ids", () => {
    expect(normalizeCodexRealtimeVoice(" Cove ")).toBe("cove");
    expect(normalizeCodexRealtimeVoice(" Himari ")).toBe("himari");
    expect(normalizeCodexRealtimeVoice(null)).toBe("cove");
    expect(
      normalizeCodexVoicePreferences({
        voice: "maple",
        inputDeviceId: " microphone ",
        outputDeviceId: "x".repeat(513),
      }),
    ).toEqual({ voice: "maple", inputDeviceId: "microphone", outputDeviceId: "" });
  });
});
