import { describe, expect, it } from "vitest";

import { splitRuntimeDockTickerText } from "../../../src/pages/chat/runtimeDockTickerText";

describe("splitRuntimeDockTickerText", () => {
  it("按句切开中英文文本并保留句末标点", () => {
    expect(
      splitRuntimeDockTickerText("先看真实运行态。再确认 queued 卡点！Then inspect the prompt bridge? Finally patch it;")
    ).toEqual([
      "先看真实运行态。",
      "再确认 queued 卡点！",
      "Then inspect the prompt bridge?",
      "Finally patch it;",
    ]);
  });

  it("保留省略号与显式换行，不产出空句", () => {
    expect(splitRuntimeDockTickerText("第一句……\n\n第二句。\n第三句")).toEqual([
      "第一句……",
      "第二句。",
      "第三句",
    ]);
  });
});
