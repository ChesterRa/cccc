import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { copyTextToClipboard } from "../../src/utils/copy";

describe("copyTextToClipboard", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("uses navigator.clipboard when available", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    await expect(copyTextToClipboard("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to execCommand and never uses window.prompt", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    const focus = vi.fn();
    const select = vi.fn();
    const setAttribute = vi.fn();
    const textarea = { value: "", style: {}, focus, select, setAttribute };
    const appendChild = vi.fn();
    const removeChild = vi.fn();
    const execCommand = vi.fn().mockReturnValue(true);
    const prompt = vi.fn();

    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.stubGlobal("window", { prompt });
    vi.stubGlobal("document", {
      body: { appendChild, removeChild },
      createElement: vi.fn().mockReturnValue(textarea),
      execCommand,
    });

    await expect(copyTextToClipboard("fallback text")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("fallback text");
    expect(setAttribute).toHaveBeenCalledWith("readonly", "true");
    expect(appendChild).toHaveBeenCalledWith(textarea);
    expect(focus).toHaveBeenCalled();
    expect(select).toHaveBeenCalled();
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(removeChild).toHaveBeenCalledWith(textarea);
    expect(prompt).not.toHaveBeenCalled();
  });
});
