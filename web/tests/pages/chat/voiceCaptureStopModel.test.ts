import { describe, expect, it } from "vitest";

import { voiceCaptureStopAction } from "../../../src/pages/chat/voice-secretary/voiceCaptureStopModel";

describe("voice capture stop model", () => {
  it("releases the local microphone immediately on user stop", () => {
    expect(voiceCaptureStopAction()).toEqual({
      releaseLocalMicrophoneNow: true,
      waitForRemoteFinalization: true,
    });
  });
});
