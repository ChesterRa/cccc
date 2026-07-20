import { describe, expect, it } from "vite-plus/test";
import { createVoiceRecordingSessionId } from "./voiceCaptureLock";

describe("voice recording session identity", () => {
  it("creates a fresh identity for every recording", () => {
    const first = createVoiceRecordingSessionId();
    const second = createVoiceRecordingSessionId();
    expect(first).toMatch(/^voice-session-/);
    expect(second).toMatch(/^voice-session-/);
    expect(second).not.toBe(first);
  });
});
