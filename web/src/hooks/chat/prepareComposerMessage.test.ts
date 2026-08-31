// @vitest-environment happy-dom

import { describe, expect, it } from "vite-plus/test";

import { MAX_INLINE_COMPOSER_TEXT_BYTES, prepareComposerMessage } from "./prepareComposerMessage";

describe("prepareComposerMessage", () => {
  it("keeps normal messages inline without mutating the selected files", () => {
    const original = new File(["evidence"], "evidence.txt", { type: "text/plain" });
    const files = [original];

    const result = prepareComposerMessage({ text: " hello ", files });

    expect(result).toEqual({ text: "hello", files: [original], converted: false });
    expect(result.files).not.toBe(files);
  });

  it("turns oversized UTF-8 text into a timestamped attachment", async () => {
    const text = "界".repeat(Math.floor(MAX_INLINE_COMPOSER_TEXT_BYTES / 3) + 1);

    const result = prepareComposerMessage({ text, files: [], now: Date.UTC(2026, 7, 28, 1, 2, 3) });

    expect(result.converted).toBe(true);
    expect(result.text).toBe("[file] cccc-message-20260828010203.txt");
    expect(result.files).toHaveLength(1);
    expect(result.files[0].name).toBe("cccc-message-20260828010203.txt");
    expect(result.files[0].type).toBe("text/plain;charset=utf-8");
    expect(await result.files[0].text()).toBe(text);
  });

  it("keeps oversized text inline when the destination cannot carry attachments", () => {
    const text = "x".repeat(MAX_INLINE_COMPOSER_TEXT_BYTES + 1);

    const result = prepareComposerMessage({
      text,
      files: [],
      targets: [{ isCrossGroup: true, isRemote: false }],
    });

    expect(result).toEqual({ text, files: [], converted: false });
  });
});
