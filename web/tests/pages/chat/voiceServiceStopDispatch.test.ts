import { describe, expect, it } from "vitest";

import { voiceServiceStopDispatchKind } from "../../../src/pages/chat/voice-secretary/voiceServiceStopDispatch";

describe("voice service stop dispatch", () => {
  it("dispatches prompt text on stop when no prompt request is pending", () => {
    expect(voiceServiceStopDispatchKind({
      mode: "prompt",
      transcriptText: "optimize this prompt",
      pendingPromptRequestId: "",
    })).toBe("prompt");
  });

  it("does not duplicate prompt or instruction dispatches with pending requests", () => {
    expect(voiceServiceStopDispatchKind({
      mode: "prompt",
      transcriptText: "optimize this prompt",
      pendingPromptRequestId: "voice-prompt-1",
    })).toBe("");
    expect(voiceServiceStopDispatchKind({
      mode: "instruction",
      transcriptText: "summarize the document",
      pendingAskRequestId: "voice-ask-1",
    })).toBe("");
  });

  it("keeps document mode on the document flush path", () => {
    expect(voiceServiceStopDispatchKind({
      mode: "document",
      transcriptText: "meeting notes",
    })).toBe("");
  });
});
