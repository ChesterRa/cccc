import { describe, expect, it } from "vitest";

import { assembleVoiceFinalTranscriptSegments } from "../../../src/pages/chat/voice-secretary/voiceFinalTranscriptAssembler";

describe("voice final transcript assembler", () => {
  it("merges short neighboring ASR chunks into a readable utterance", () => {
    const utterances = assembleVoiceFinalTranscriptSegments([
      { text: "这个阶段我们先用 rain 作为 scaffold", startMs: 0, endMs: 1200 },
      { text: "再让 agent 处理 language understanding", startMs: 1500, endMs: 3200 },
      { text: "这样上下文会完整很多", startMs: 3500, endMs: 5000 },
    ]);

    expect(utterances).toEqual([
      {
        text: "这个阶段我们先用 rain 作为 scaffold 再让 agent 处理 language understanding 这样上下文会完整很多",
        startMs: 0,
        endMs: 5000,
      },
    ]);
  });

  it("starts a new utterance after a long pause", () => {
    const utterances = assembleVoiceFinalTranscriptSegments([
      { text: "第一句已经讲完了", startMs: 0, endMs: 1200 },
      { text: "第二句隔了很久才开始", startMs: 4800, endMs: 6200 },
    ], { minChars: 6 });

    expect(utterances.map((item) => item.text)).toEqual([
      "第一句已经讲完了",
      "第二句隔了很久才开始",
    ]);
  });

  it("respects terminal punctuation when the current utterance is long enough", () => {
    const utterances = assembleVoiceFinalTranscriptSegments([
      { text: "这里先讲完一个完整观点。", startMs: 0, endMs: 1600 },
      { text: "然后再开始下一个观点", startMs: 1800, endMs: 3200 },
    ], { minChars: 8 });

    expect(utterances.map((item) => item.text)).toEqual([
      "这里先讲完一个完整观点。",
      "然后再开始下一个观点",
    ]);
  });
});
