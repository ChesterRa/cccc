import { describe, expect, it } from "vitest";

import {
  annotateVoiceTranscriptItemsWithSpeakers,
  appendFinalVoiceTranscriptItem,
  createVoiceTranscriptItem,
  filterVoiceTranscriptItemsForDocument,
  mergeVoiceTranscriptItems,
  type VoiceTranscriptItem,
} from "../../../src/pages/chat/voice-secretary/voiceStreamModel";
import { voiceTranscriptSourceDetail, voiceTranscriptSourceLabel } from "../../../src/pages/chat/voice-secretary/voiceTranscriptSource";

function makeTranscriptItem(overrides: Partial<VoiceTranscriptItem> = {}): VoiceTranscriptItem {
  return {
    id: "item-1",
    phase: "final",
    text: "doc transcript",
    mode: "document",
    documentPath: "docs/voice-secretary/one.md",
    createdAt: 1000,
    updatedAt: 1000,
    ...overrides,
  };
}

describe("voice transcript model", () => {
  it("creates persisted transcript items only for document mode with a document path", () => {
    expect(createVoiceTranscriptItem({
      id: "ask",
      cleanText: "ask transcript",
      metadata: { mode: "instruction", documentPath: "docs/voice-secretary/one.md" },
      now: 1000,
    })).toBeNull();

    expect(createVoiceTranscriptItem({
      id: "missing-doc",
      cleanText: "document transcript",
      metadata: { mode: "document" },
      now: 1000,
    })).toBeNull();

    const item = createVoiceTranscriptItem({
      id: "doc",
      cleanText: "document transcript",
      metadata: { mode: "document", documentPath: "docs/voice-secretary/one.md" },
      now: 1000,
    });

    expect(item?.id).toBe("doc");
    expect(item?.mode).toBe("document");
    expect(item?.documentPath).toBe("docs/voice-secretary/one.md");
  });

  it("keeps transcript source metadata on persisted items", () => {
    const item = createVoiceTranscriptItem({
      id: "doc",
      cleanText: "document transcript",
      metadata: {
        mode: "document",
        documentPath: "docs/voice-secretary/one.md",
        source: "assistant_service_local_asr_final",
        sourceLabel: "Final SenseVoice",
        sourceDetail: "sense_voice · lang=auto · 2 chunks",
      },
      now: 1000,
    });

    expect(item?.sourceLabel).toBe("Final SenseVoice");
    expect(item?.sourceDetail).toBe("sense_voice · lang=auto · 2 chunks");
  });

  it("filters transcript rows to the selected document path", () => {
    const items = [
      makeTranscriptItem({ id: "first-doc", text: "first", documentPath: "docs/voice-secretary/first.md" }),
      makeTranscriptItem({ id: "second-doc", text: "second", documentPath: "docs/voice-secretary/second.md", updatedAt: 2000 }),
    ];

    expect(filterVoiceTranscriptItemsForDocument(items, "docs/voice-secretary/second.md").map((item) => item.id)).toEqual(["second-doc"]);
    expect(filterVoiceTranscriptItemsForDocument(items, "")).toEqual([]);
  });

  it("replaces a live row when the final transcript for the same window arrives", () => {
    const live = makeTranscriptItem({ id: "live", phase: "interim", text: "temporary", updatedAt: 1000 });
    const final = makeTranscriptItem({ id: "final", text: "final text", updatedAt: 2000 });

    const items = appendFinalVoiceTranscriptItem([live], final, "live");

    expect(items.map((item) => item.id)).toEqual(["final"]);
    expect(items[0]?.text).toBe("final text");
  });

  it("dedupes restored and local transcript rows for the same document", () => {
    const local = makeTranscriptItem({ id: "local", text: "same transcript", updatedAt: 1000 });
    const restored = makeTranscriptItem({ id: "restored", text: "same transcript", updatedAt: 1500 });

    const items = mergeVoiceTranscriptItems([local], [restored]);

    expect(items.map((item) => item.id)).toEqual(["restored"]);
  });

  it("does not force a single speaker badge on mixed-speaker transcript rows", () => {
    const item = makeTranscriptItem({
      startMs: 0,
      endMs: 10_000,
      speakerLabel: "Speaker 4",
      speakerIndex: 3,
    });

    const items = annotateVoiceTranscriptItemsWithSpeakers([item], [
      { start_ms: 0, end_ms: 5_000, speaker_label: "Speaker 1", speaker_index: 0 },
      { start_ms: 5_000, end_ms: 10_000, speaker_label: "Speaker 2", speaker_index: 1 },
    ]);

    expect(items[0]?.speakerLabel).toBeUndefined();
    expect(items[0]?.speakerIndex).toBeUndefined();
  });

  it("keeps a speaker badge when one speaker clearly dominates the transcript row", () => {
    const item = makeTranscriptItem({ startMs: 0, endMs: 10_000 });

    const items = annotateVoiceTranscriptItemsWithSpeakers([item], [
      { start_ms: 0, end_ms: 8_000, speaker_label: "Speaker 1", speaker_index: 0 },
      { start_ms: 8_000, end_ms: 10_000, speaker_label: "Speaker 2", speaker_index: 1 },
    ]);

    expect(items[0]?.speakerLabel).toBe("Speaker 1");
    expect(items[0]?.speakerIndex).toBe(0);
  });

  it("formats source labels and compact source details", () => {
    expect(voiceTranscriptSourceLabel("assistant_service_local_asr_final")).toBe("Final SenseVoice");
    expect(voiceTranscriptSourceDetail({
      modelId: "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8",
      engine: "sense_voice",
      language: "auto",
      chunks: 2,
      fallbackReason: "vad_failed",
    })).toBe("sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8 · sense_voice · lang=auto · 2 chunks · fallback=vad_failed");
  });
});
