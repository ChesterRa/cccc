import { describe, expect, it } from "vitest";

import {
  appendVoiceActivityStreamItem,
  filterVisibleVoiceActivityStreamItems,
  newestVoiceActivityItemsFirst,
  shouldSettleLiveVoiceActivityStream,
  voiceActivityStreamItemFromPreview,
} from "../../../src/pages/chat/voice-secretary/voiceActivityStreamModel";

describe("voice activity stream model", () => {
  it("updates the active stream card instead of duplicating partial hypotheses", () => {
    const first = appendVoiceActivityStreamItem([], {
      id: "stream-1",
      phase: "interim",
      text: "Speaker ?: hello",
      mode: "document",
      documentPath: "docs/voice-secretary/one.md",
      updatedAt: 1000,
    });
    const second = appendVoiceActivityStreamItem(first, {
      id: "stream-1",
      phase: "interim",
      text: "Speaker ?: hello world",
      mode: "document",
      documentPath: "docs/voice-secretary/one.md",
      updatedAt: 2000,
    });

    expect(second.map((item) => item.text)).toEqual(["hello world"]);
    expect(second.every((item) => !item.text.includes("Speaker ?:"))).toBe(true);
  });

  it("updates a continued stream card even when a new capture stream id is created", () => {
    const first = appendVoiceActivityStreamItem([], {
      id: "stream-1",
      phase: "final",
      text: "我们大学刚毕业的时候去了一个公司实习",
      mode: "document",
      updatedAt: 1000,
    });
    const second = appendVoiceActivityStreamItem(first, {
      id: "stream-2",
      phase: "interim",
      text: "我们大学刚毕业的时候去了一个公司实习第一",
      mode: "document",
      updatedAt: 2000,
    });

    expect(second.map((item) => item.text)).toEqual(["我们大学刚毕业的时候去了一个公司实习第一"]);
  });

  it("keeps revised longer partial text in the same activity card", () => {
    const first = appendVoiceActivityStreamItem([], {
      id: "stream-1",
      phase: "interim",
      text: "我这里有另外一个词很热就是 continu 的的 arning for jao online learning 这个会是你定义的新学学习是吗你觉得",
      mode: "document",
      updatedAt: 1000,
    });
    const second = appendVoiceActivityStreamItem(first, {
      id: "stream-1",
      phase: "interim",
      text: "我这里有另外一个词很热就是 continu 的的 arning for jao online learning 这个会是你定义的新学学习是吗你觉得可能这个事",
      mode: "document",
      updatedAt: 2000,
    });

    expect(second).toHaveLength(1);
    expect(second[0]?.text).toContain("可能这个事");
  });

  it("keeps stream items while recording and ages them out after recording stops", () => {
    const items = appendVoiceActivityStreamItem([], {
      id: "stream-1",
      phase: "interim",
      text: "hello",
      mode: "document",
      updatedAt: 1000,
    });

    expect(filterVisibleVoiceActivityStreamItems(items, 10_000, 1_000, true)).toHaveLength(1);
    expect(filterVisibleVoiceActivityStreamItems(items, 10_000, 1_000, false)).toHaveLength(0);
  });

  it("builds a current live activity item from the transient preview", () => {
    const item = voiceActivityStreamItemFromPreview({
      id: "stream-1",
      phase: "interim",
      text: "Speaker ?: live text",
      mode: "instruction",
      updatedAt: 1000,
    });

    expect(item?.id).toBe("stream-1-live");
    expect(item?.text).toBe("live text");
    expect(item?.mode).toBe("instruction");
  });

  it("settles the current live card before a new short hypothesis replaces it", () => {
    expect(shouldSettleLiveVoiceActivityStream({
      id: "stream-1",
      phase: "interim",
      text: "可某一路到后 mosapiens 的这个这个这个快速通道所以语言它远远不是或者是以语言为代表整个符",
      mode: "document",
      updatedAt: 1000,
    }, "最为", "interim")).toBe(true);
  });

  it("does not settle the current live card for normal extended partial text", () => {
    expect(shouldSettleLiveVoiceActivityStream({
      id: "stream-1",
      phase: "interim",
      text: "最为",
      mode: "document",
      updatedAt: 1000,
    }, "最为关键", "interim")).toBe(false);
  });

  it("keeps recent activity ordered newest first", () => {
    expect(newestVoiceActivityItemsFirst([
      { id: "old", sortAt: 1000 },
      { id: "new", sortAt: 3000 },
      { id: "middle", sortAt: 2000 },
    ], 2).map((item) => item.id)).toEqual(["new", "middle"]);
  });
});
