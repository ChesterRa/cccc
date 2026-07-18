import { describe, expect, it } from "vite-plus/test";
import {
  getMessageBubbleMotionClass as getMotionClass,
  mayContainMarkdown as mayRenderMarkdown,
} from "../../src/components/messageBubble/helpers";

describe("getMessageBubbleMotionClass", () => {
  it("animates commentary bubbles with the transient commentary class", () => {
    expect(
      getMotionClass({ isStreaming: true, isOptimistic: false, streamPhase: "commentary" }),
    ).toBe("cccc-transient-bubble cccc-transient-bubble-commentary");
  });

  it("animates optimistic bubbles with the base transient class", () => {
    expect(getMotionClass({ isStreaming: false, isOptimistic: true, streamPhase: "" })).toBe(
      "cccc-transient-bubble",
    );
  });

  it("keeps stable final bubbles animation-free", () => {
    expect(
      getMotionClass({ isStreaming: false, isOptimistic: false, streamPhase: "final_answer" }),
    ).toBe("");
  });
});

describe("mayContainMarkdown", () => {
  it("detects GitHub-style tables so completed chat bubbles render markdown tables", () => {
    expect(
      mayRenderMarkdown(
        [
          "本周天气如下：",
          "",
          "| 日期 | 天气 | 温度 |",
          "| --- | --- | --- |",
          "| 周一 | 晴 | 24°C |",
          "| 周二 | 多云 | 22°C |",
        ].join("\n"),
      ),
    ).toBe(true);
  });

  it("keeps internal attachment manifests as plain text", () => {
    expect(mayRenderMarkdown("[cccc] Attachments:\n- file.txt")).toBe(false);
  });
});
