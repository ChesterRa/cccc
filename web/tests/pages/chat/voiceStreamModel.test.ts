import { describe, expect, it } from "vitest";

import {
  appendFinalVoiceStreamItem,
  buildSpeakerConversationItems,
  createVoiceStreamMessage,
  createVoiceTranscriptPreview,
  filterVoiceStreamItemsForDocument,
  splitVoiceStreamItemBySpeakers,
  upsertLiveVoiceStreamItem,
  type VoiceStreamItem,
} from "../../../src/pages/chat/voice-secretary/voiceStreamModel";

function makeStreamItem(overrides: Partial<VoiceStreamItem> = {}): VoiceStreamItem {
  return {
    id: "item-1",
    phase: "final",
    text: "你好我们开始讨论后续方案",
    mode: "document",
    startMs: 0,
    endMs: 6000,
    createdAt: 1000,
    updatedAt: 1000,
    ...overrides,
  };
}

describe("voiceStreamModel", () => {
  it("splits one transcript item across multiple speaker segments", () => {
    const items = splitVoiceStreamItemBySpeakers(makeStreamItem(), [
      { speaker_label: "Speaker 1", start_ms: 0, end_ms: 3000 },
      { speaker_label: "Speaker 2", start_ms: 3000, end_ms: 6000 },
    ]);

    expect(items).toHaveLength(2);
    expect(items.map((item) => item.speakerLabel)).toEqual(["Speaker 1", "Speaker 2"]);
    expect(items.map((item) => item.text).join("")).toBe("你好我们开始讨论后续方案");
    expect(items.map((item) => [item.startMs, item.endMs])).toEqual([[0, 3000], [3000, 6000]]);
  });

  it("keeps uncovered text as an unspeaked heard slice instead of dropping it", () => {
    const items = splitVoiceStreamItemBySpeakers(makeStreamItem({ text: "ab cd ef", startMs: 0, endMs: 6000 }), [
      { speaker_label: "Speaker 1", start_ms: 2000, end_ms: 4000 },
    ]);

    expect(items.map((item) => item.speakerLabel)).toEqual(["", "Speaker 1", ""]);
    expect(items.map((item) => item.text).join("")).toBe("ab cd ef");
  });

  it("puts live and newest voice stream entries first for stream rendering", () => {
    const older = createVoiceStreamMessage({
      id: "older",
      cleanText: "先说",
      metadata: { mode: "document" },
      timing: { startMs: 0, endMs: 1000 },
      now: 1000,
    });
    const newer = createVoiceStreamMessage({
      id: "newer",
      cleanText: "后说",
      metadata: { mode: "document" },
      timing: { startMs: 1000, endMs: 2000 },
      now: 2000,
    });
    const live = {
      ...createVoiceTranscriptPreview({
        id: "live",
        cleanText: "正在说",
        phase: "interim" as const,
        pendingFinalText: "",
        metadata: { mode: "document" },
        timing: { startMs: 2000, endMs: 3000 },
        now: 1500,
      }),
      createdAt: 1500,
    };

    const items = buildSpeakerConversationItems([older, live, newer], []);

    expect(items.map((item) => item.text)).toEqual(["正在说", "先说后说"]);
  });

  it("coalesces adjacent short heard transcript rows for display", () => {
    const first = makeStreamItem({
      id: "first",
      text: "但是这个",
      startMs: 40_000,
      endMs: 41_000,
      createdAt: 1000,
      updatedAt: 1000,
    });
    const second = makeStreamItem({
      id: "second",
      text: "对美国肯定是挑战",
      startMs: 41_000,
      endMs: 44_000,
      createdAt: 2000,
      updatedAt: 2000,
    });

    const items = buildSpeakerConversationItems([second, first], []);

    expect(items).toHaveLength(1);
    expect(items[0]?.text).toBe("但是这个对美国肯定是挑战");
  });

  it("does not use diarization time slices to split raw stream text", () => {
    const item = makeStreamItem({ text: "严社会呢一个多亿的中产对吧程序员律师医生中", startMs: 46_000, endMs: 52_000 });

    const items = buildSpeakerConversationItems(
      [item],
      [
        { speaker_label: "Speaker 3", start_ms: 46_000, end_ms: 52_000 },
        { speaker_label: "Speaker 7", start_ms: 52_000, end_ms: 53_000 },
      ],
    );

    expect(items).toHaveLength(1);
    expect(items[0]?.speakerLabel).toBe("");
    expect(items[0]?.text).toBe("严社会呢一个多亿的中产对吧程序员律师医生中");
  });

  it("uses backend speaker transcript segments instead of guessing a front-end split", () => {
    const item = makeStreamItem({ text: "这是一整段不会被前端猜切的文本" });

    const items = buildSpeakerConversationItems(
      [item],
      [
        { speaker_label: "Speaker 1", start_ms: 0, end_ms: 3000 },
        { speaker_label: "Speaker 2", start_ms: 3000, end_ms: 6000 },
      ],
      [
        { speaker_label: "Speaker 1", start_ms: 0, end_ms: 3000, text: "真实第一段" },
        { speaker_label: "Speaker 2", start_ms: 3000, end_ms: 6000, text: "真实第二段" },
      ],
    );

    expect(items.map((row) => `${row.speakerLabel}:${row.text}`)).toEqual([
      "Speaker 2:真实第二段",
      "Speaker 1:真实第一段",
    ]);
  });

  it("uses backend speaker transcript segments only when stream text is unavailable", () => {
    const items = buildSpeakerConversationItems(
      [],
      [],
      [
        { speaker_label: "Speaker 1", start_ms: 0, end_ms: 3000, text: "第一段" },
        { speaker_label: "Speaker 2", start_ms: 3000, end_ms: 6000, text: "第二段" },
      ],
    );

    expect(items.map((row) => `${row.speakerLabel}:${row.text}`)).toEqual([
      "Speaker 2:第二段",
      "Speaker 1:第一段",
    ]);
  });

  it("replaces the live preview item when a final message is appended", () => {
    const livePreview = createVoiceTranscriptPreview({
      id: "live",
      cleanText: "临时",
      phase: "interim",
      pendingFinalText: "",
      metadata: { mode: "document" },
      now: 1000,
    });
    const liveItems = upsertLiveVoiceStreamItem([], livePreview);
    const finalItem = createVoiceStreamMessage({
      id: "final",
      cleanText: "最终",
      metadata: { mode: "document" },
      now: 2000,
    });

    const items = appendFinalVoiceStreamItem(liveItems, finalItem, "live");

    expect(items.map((item) => item.id)).toEqual(["final"]);
    expect(items[0]?.text).toBe("最终");
  });

  it("does not append duplicate final transcript items for the same audio window", () => {
    const existing = makeStreamItem({
      id: "persisted-1",
      text: "重复的一段会议文本",
      startMs: 0,
      endMs: 7000,
      createdAt: 1000,
      updatedAt: 1000,
    });
    const duplicate = makeStreamItem({
      id: "final-1",
      text: "重复的一段会议文本",
      startMs: 80,
      endMs: 7100,
      createdAt: 2000,
      updatedAt: 2000,
    });

    const items = appendFinalVoiceStreamItem([existing], duplicate);

    expect(items.map((item) => item.id)).toEqual(["final-1"]);
  });

  it("dedupes duplicate stream rows before speaker rendering", () => {
    const first = makeStreamItem({
      id: "first",
      text: "同一段内容",
      startMs: 0,
      endMs: 7000,
      createdAt: 1000,
      updatedAt: 1000,
    });
    const second = makeStreamItem({
      id: "second",
      text: "同一段内容",
      startMs: 0,
      endMs: 7000,
      createdAt: 2000,
      updatedAt: 2000,
    });

    const items = buildSpeakerConversationItems([first, second], []);

    expect(items.map((item) => item.sourceItemId)).toEqual(["second"]);
  });

  it("filters stream rows to the selected document path", () => {
    const firstDoc = makeStreamItem({
      id: "first-doc",
      text: "第一个文档",
      documentPath: "docs/voice-secretary/first.md",
    });
    const secondDoc = makeStreamItem({
      id: "second-doc",
      text: "第二个文档",
      documentPath: "docs/voice-secretary/second.md",
    });

    const items = filterVoiceStreamItemsForDocument(
      [firstDoc, secondDoc],
      "docs/voice-secretary/second.md",
    );

    expect(items.map((item) => item.id)).toEqual(["second-doc"]);
  });
});
