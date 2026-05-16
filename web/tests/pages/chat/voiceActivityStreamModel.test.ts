import { describe, expect, it } from "vitest";

import {
  newestVoiceActivityItemsFirst,
  shouldSettleLiveVoiceActivityStream,
  voiceActivityStreamItemFromPreview,
} from "../../../src/pages/chat/voice-secretary/voiceActivityStreamModel";

describe("voice activity stream model", () => {
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

  it("does not settle a final live card when the next interim continues the same text", () => {
    expect(shouldSettleLiveVoiceActivityStream({
      id: "stream-1",
      phase: "final",
      text: "Stop the recording in",
      mode: "prompt",
      updatedAt: 1000,
    }, "Stop the recording in CCCC", "interim")).toBe(false);
  });

  it("keeps recent activity ordered newest first", () => {
    expect(newestVoiceActivityItemsFirst([
      { id: "old", sortAt: 1000 },
      { id: "new", sortAt: 3000 },
      { id: "middle", sortAt: 2000 },
    ], 2).map((item) => item.id)).toEqual(["new", "middle"]);
  });
});
