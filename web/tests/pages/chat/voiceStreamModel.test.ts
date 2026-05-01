import { describe, expect, it } from "vitest";

import {
  appendFinalVoiceTranscriptItem,
  createVoiceTranscriptItem,
  createVoiceTranscriptPreview,
  filterVoiceTranscriptItemsForDocument,
  mergeVoiceTranscriptItems,
  upsertLiveVoiceTranscriptItem,
  type VoiceTranscriptItem,
} from "../../../src/pages/chat/voice-secretary/voiceStreamModel";

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

  it("keeps live transcript previews out of the doc transcript list unless they target a document", () => {
    const askPreview = createVoiceTranscriptPreview({
      id: "ask-live",
      cleanText: "ask live text",
      phase: "interim",
      pendingFinalText: "",
      metadata: { mode: "instruction", documentPath: "docs/voice-secretary/one.md" },
      now: 1000,
    });
    expect(upsertLiveVoiceTranscriptItem([], askPreview)).toEqual([]);

    const docPreview = createVoiceTranscriptPreview({
      id: "doc-live",
      cleanText: "doc live text",
      phase: "interim",
      pendingFinalText: "",
      metadata: { mode: "document", documentPath: "docs/voice-secretary/one.md" },
      now: 2000,
    });
    expect(upsertLiveVoiceTranscriptItem([], docPreview).map((item) => item.id)).toEqual(["doc-live"]);
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
});
