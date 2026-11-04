import { describe, expect, it } from "vite-plus/test";

import { shouldCloseVoiceCaptureSocket } from "../../../src/pages/chat/voice-secretary/voiceCaptureStopModel";

describe("voice capture stop model", () => {
  it("closes both connecting and open capture sockets during cleanup", () => {
    expect(shouldCloseVoiceCaptureSocket(0)).toBe(true);
    expect(shouldCloseVoiceCaptureSocket(1)).toBe(true);
    expect(shouldCloseVoiceCaptureSocket(2)).toBe(false);
    expect(shouldCloseVoiceCaptureSocket(3)).toBe(false);
  });
});
