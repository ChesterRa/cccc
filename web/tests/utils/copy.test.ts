// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { copyTextToClipboard } from "../../src/utils/copy";

describe("copyTextToClipboard", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
    Reflect.deleteProperty(window, "prompt");
    Reflect.deleteProperty(document, "execCommand");
  });

  it("uses navigator.clipboard when available", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    await expect(copyTextToClipboard("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to execCommand and never uses window.prompt", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    const dialog = document.createElement("section");
    dialog.setAttribute("role", "dialog");
    const trigger = document.createElement("button");
    dialog.append(trigger);
    document.body.append(dialog);
    trigger.focus();
    let copied = "";
    // Only the native clipboard boundary is mocked; focus and selection use DOM.
    const execCommand = vi.fn(() => {
      const field = document.activeElement as HTMLTextAreaElement;
      expect(field).toBeInstanceOf(HTMLTextAreaElement);
      expect(dialog.contains(field)).toBe(true);
      expect(field.readOnly).toBe(true);
      copied = field.value.slice(field.selectionStart, field.selectionEnd);
      return true;
    });
    const prompt = vi.fn();
    Object.defineProperty(window, "prompt", { value: prompt, configurable: true });
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });

    await expect(copyTextToClipboard("fallback text")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("fallback text");
    expect(copied).toBe("fallback text");
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(dialog.querySelector("textarea")).toBeNull();
    expect(prompt).not.toHaveBeenCalled();
  });
});
