import { describe, expect, it } from "vite-plus/test";
import { resolveVoiceComposerActivity } from "./voiceComposerActivity";

const idle = {
  recording: false,
  promptWaiting: false,
  promptReady: false,
  askSummary: "",
  transcriptSummary: "",
};

describe("voice composer activity", () => {
  it("shows the recording target before any background result", () => {
    expect(
      resolveVoiceComposerActivity({
        recording: true,
        promptWaiting: true,
        promptReady: false,
        askSummary: "Working",
        transcriptSummary: "hello",
      }),
    ).toBe("recording");
  });

  it("orders prompt progress before ask replies and old transcript", () => {
    expect(
      resolveVoiceComposerActivity({ ...idle, promptWaiting: true, askSummary: "Reply ready" }),
    ).toBe("promptWaiting");
    expect(
      resolveVoiceComposerActivity({ ...idle, promptReady: true, transcriptSummary: "x" }),
    ).toBe("promptReady");
    expect(
      resolveVoiceComposerActivity({ ...idle, askSummary: "Reply ready", transcriptSummary: "x" }),
    ).toBe("ask");
    expect(resolveVoiceComposerActivity({ ...idle, transcriptSummary: "x" })).toBe("transcript");
    expect(resolveVoiceComposerActivity(idle)).toBe("");
  });
});
