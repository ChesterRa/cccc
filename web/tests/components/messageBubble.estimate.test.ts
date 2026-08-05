import { describe, expect, it } from "vite-plus/test";

import { estimateMessageRowHeight } from "../../src/components/messageBubble/estimate";
import {
  getMessageImageGridHeight,
  MESSAGE_ATTACHMENT_SECTION_LEADING_PX,
  MESSAGE_IMAGE_PREVIEW_HEIGHT_PX,
} from "../../src/components/messageBubble/imageLayout";
import type { LedgerEvent } from "../../src/types";

describe("estimateMessageRowHeight", () => {
  it("returns queued placeholder height for pure queued streaming messages", () => {
    const message: LedgerEvent = {
      _streaming: true,
      data: {
        text: "",
        attachments: [],
        activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
      },
    };

    expect(estimateMessageRowHeight(message)).toBe(84);
  });

  it("returns a larger baseline for non-placeholder streaming messages", () => {
    const message: LedgerEvent = {
      _streaming: true,
      data: {
        text: "我正在查消息滚动问题",
        activities: [{ id: "thinking:1", kind: "thinking", status: "started", summary: "分析中" }],
      },
    };

    expect(estimateMessageRowHeight(message)).toBeGreaterThan(108);
  });

  it("adds quoted reply and attachments for canonical messages", () => {
    const message: LedgerEvent = {
      data: {
        text: "带附件和引用的消息",
        quote_text: "上一条消息",
        attachments: [{ mime_type: "image/png" }, { mime_type: "application/pdf" }],
      },
    };

    expect(estimateMessageRowHeight(message)).toBeGreaterThan(72 + 60 + 200 + 48);
  });

  it("uses the fixed rendered preview height for one image", () => {
    const withoutImage = estimateMessageRowHeight({ data: { text: "same text" } });
    const withImage = estimateMessageRowHeight({
      data: { text: "same text", attachments: [{ mime_type: "image/png" }] },
    });

    expect(withImage - withoutImage).toBe(
      MESSAGE_ATTACHMENT_SECTION_LEADING_PX + MESSAGE_IMAGE_PREVIEW_HEIGHT_PX.hero,
    );
  });

  it("matches fixed two-column grid rows without charging a trailing gap", () => {
    expect(getMessageImageGridHeight(2)).toBe(MESSAGE_IMAGE_PREVIEW_HEIGHT_PX.grid);
    expect(getMessageImageGridHeight(3)).toBe(MESSAGE_IMAGE_PREVIEW_HEIGHT_PX.grid * 2 + 8);
  });

  it("adds extra code block height for markdown code fences", () => {
    const normal = estimateMessageRowHeight({ data: { text: "普通文本" } });
    const withCode = estimateMessageRowHeight({ data: { text: "```ts\nconst x = 1;\n```" } });

    expect(withCode).toBeGreaterThan(normal);
  });
});
