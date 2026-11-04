import { describe, expect, it } from "vite-plus/test";

import {
  isMeaningfulVoiceDispatchText,
  isVoiceDictationText,
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
    expect(
      voiceServiceStopDispatchKind({
        mode: "prompt",
        transcriptText: "optimize this prompt",
        pendingPromptRequestId: "",
      }),
    ).toBe("prompt");
  });

  it("keeps a later prompt editable while a refinement is pending", () => {
    expect(
      voiceServiceStopDispatchKind({
        mode: "prompt",
        transcriptText: "optimize this prompt",
        pendingPromptRequestId: "voice-prompt-1",
      }),
    ).toBe("");
    expect(
      voiceServiceStopDispatchKind({
        mode: "instruction",
        transcriptText: "summarize the document",
      }),
    ).toBe("instruction");
  });

  it("keeps document mode on the document flush path", () => {
    expect(
      voiceServiceStopDispatchKind({ mode: "document", transcriptText: "meeting notes" }),
    ).toBe("");
  });

  it("keeps every short answer for direct dictation", () => {
    for (const answer of [
      "好。",
      "对",
      "嗯",
      "哦",
      "OK.",
      "I.",
      "No.",
      "はい",
      "네",
      "да",
      "à",
      "é",
      "Sí",
      "3",
      "+",
      "×",
      "😊",
    ]) {
      expect(isVoiceDictationText(answer), answer).toBe(true);
    }
    for (const blank of ["", "   ", "。", "...", "?!", "、"]) {
      expect(isVoiceDictationText(blank), blank).toBe(false);
    }
  });

  it("does not dispatch prompt or instruction for ASR noise", () => {
    expect(
      voiceServiceStopDispatchKind({
        mode: "prompt",
        transcriptText: "I.",
        pendingPromptRequestId: "",
      }),
    ).toBe("");
    expect(
      voiceServiceStopDispatchKind({ mode: "instruction", transcriptText: "The. Yeah." }),
    ).toBe("");
  });
});
