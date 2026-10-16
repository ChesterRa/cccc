import { describe, expect, it } from "vitest";
import { RealtimeTranscriptAccumulator, realtimeTranscriptUpdate } from "./codexVoiceProtocol";

describe("Voice task handoff context", () => {
  it("retains the antecedent and limits, and freezes a delegation before later corrections", () => {
    const speech = new RealtimeTranscriptAccumulator();
    speech.apply({
      role: "user",
      text: "A组 Kiro 启动失败；只查原因，不改配置，不重启。",
      final: true,
      turnId: "u1",
    });
    speech.apply({
      role: "assistant",
      text: "你要查启动失败的原因吗？",
      final: true,
      turnId: "a1",
    });
    speech.apply({ role: "user", text: "对，就查这个", final: false, turnId: "u2" });
    const first = speech.handoffContext("codex:d1");
    expect(first.messages.map((message) => message.text)).toEqual([
      "A组 Kiro 启动失败；只查原因，不改配置，不重启。",
      "你要查启动失败的原因吗？",
      "对，就查这个",
    ]);
    expect(first.messages.at(-1)?.incomplete).toBe(true);
    speech.apply({ role: "user", text: "改成B组，仍只查不改。", final: true, turnId: "u2" });
    expect(speech.handoffContext("codex:d1")).toEqual(first);
    expect(speech.handoffContext("codex:d2").messages.at(-1)).toEqual({
      role: "user",
      text: "改成B组，仍只查不改。",
      incomplete: false,
    });
    expect(new RealtimeTranscriptAccumulator().handoffContext("codex:d1")).toEqual({
      messages: [],
      truncated: false,
    });
  });

  it("bounds UTF-8 context without corrupting multilingual text or hiding omissions", () => {
    const speech = new RealtimeTranscriptAccumulator();
    for (let index = 0; index < 16; index++) {
      speech.apply({
        role: index % 2 ? "assistant" : "user",
        text: `第${index}段。` + "界🙂".repeat(800),
        final: true,
        turnId: String(index),
      });
    }
    const context = speech.handoffContext("grok:call-1");
    const encoder = new TextEncoder();
    expect(context.messages.length).toBeLessThanOrEqual(12);
    expect(
      context.messages.reduce((size, message) => size + encoder.encode(message.text).length, 0),
    ).toBeLessThanOrEqual(8192);
    expect(context.truncated).toBe(true);
    for (const message of context.messages) {
      expect(new TextDecoder().decode(encoder.encode(message.text))).toBe(message.text);
    }
    expect(context.messages.at(-1)?.text).toContain("第15段。");
    expect(context.messages[0].incomplete).toBe(true);
  });

  it("marks display clipping and keeps surrogate pairs valid in a Codex transcript", () => {
    const raw = "start🙂" + "a".repeat(3999);
    const update = realtimeTranscriptUpdate({
      type: "turn.done",
      turn: { id: "long", role: "user", transcript: raw },
    });
    expect(update?.truncated).toBe(true);
    const speech = new RealtimeTranscriptAccumulator();
    speech.apply(update!);
    const context = speech.handoffContext("codex:long");
    expect(context.truncated).toBe(true);
    const text = context.messages[0].text;
    expect(new TextDecoder().decode(new TextEncoder().encode(text))).toBe(text);
  });

  it("retains at most the recent twelve messages and marks an older constraint as omitted", () => {
    const speech = new RealtimeTranscriptAccumulator();
    for (let index = 0; index < 14; index++) {
      speech.apply({ role: "user", text: `message-${index}`, final: true, turnId: String(index) });
    }
    const context = speech.handoffContext("request");
    expect(context.messages).toHaveLength(12);
    expect(context.messages[0].text).toBe("message-2");
    expect(context.truncated).toBe(true);
  });
});
