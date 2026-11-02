import { describe, expect, it } from "vite-plus/test";
import { mayContainMarkdown as mayRenderMarkdown } from "../../src/components/messageBubble/helpers";

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
