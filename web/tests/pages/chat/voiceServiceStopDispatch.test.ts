import { describe, expect, it } from "vitest";

import {
  isMeaningfulVoiceDispatchText,
  voiceServiceStopDispatchKind,
} from "../../../src/pages/chat/voice-secretary/voiceServiceStopDispatch";

describe("voice service stop dispatch", () => {
  it("filters ASR noise fragments before dispatch", () => {
    expect(isMeaningfulVoiceDispatchText("")).toBe(false);
    expect(isMeaningfulVoiceDispatchText(".")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("I.")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("嗯。")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("The.")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("The. Yeah.")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("Yeah. Yes.")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("OK.")).toBe(false);
    expect(isMeaningfulVoiceDispatchText("帮我查天气")).toBe(true);
    expect(isMeaningfulVoiceDispatchText("summarize this")).toBe(true);
    expect(isMeaningfulVoiceDispatchText("the weather")).toBe(true);
  });

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

  it("does not dispatch prompt or instruction for ASR noise", () => {
    expect(voiceServiceStopDispatchKind({
      mode: "prompt",
      transcriptText: "I.",
      pendingPromptRequestId: "",
    })).toBe("");
    expect(voiceServiceStopDispatchKind({
      mode: "instruction",
      transcriptText: "The. Yeah.",
      pendingAskRequestId: "",
    })).toBe("");
  });
});
