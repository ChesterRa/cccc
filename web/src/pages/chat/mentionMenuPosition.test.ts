// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { getComposerMenuAnchorRect, getMentionMenuLeft } from "./mentionMenuPosition";

describe("getMentionMenuLeft", () => {
  it("anchors the menu near the current @ trigger instead of the composer start", () => {
    expect(getMentionMenuLeft({ triggerX: 720, containerWidth: 900, menuWidth: 320 })).toBe(564);
  });

  it("keeps the menu inside the composer when the trigger is near the left edge", () => {
    expect(getMentionMenuLeft({ triggerX: 4, containerWidth: 900, menuWidth: 320 })).toBe(8);
  });

  it("keeps the menu inside the composer when the trigger is near the right edge", () => {
    expect(getMentionMenuLeft({ triggerX: 880, containerWidth: 900, menuWidth: 320 })).toBe(572);
  });
});

describe("getComposerMenuAnchorRect", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.body.replaceChildren();
  });

  function input(value: string, caret: number) {
    const box = document.createElement("div");
    box.setAttribute("data-composer-input", "");
    const textarea = document.createElement("textarea");
    box.append(textarea);
    document.body.append(box);
    textarea.value = value;
    textarea.setSelectionRange(caret, caret);
    vi.spyOn(box, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 500, 900, 80));
    Object.defineProperty(textarea, "clientWidth", { value: 900 });
    // Ten pixels per character stands in for the composer font.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      font: "",
      measureText: (text: string) => ({ width: text.length * 10 }),
    } as unknown as CanvasRenderingContext2D);
    return textarea;
  }

  it("opens above the input where the mention menu would for the insertion point", () => {
    const draft = `first line\n${"x".repeat(60)}`;
    const atEnd = getComposerMenuAnchorRect({
      textarea: input(draft, draft.length),
      wide: false,
      menuWidth: 320,
    });
    expect([atEnd.left, atEnd.top, atEnd.width]).toEqual([100 + 444, 500, 0]);
    document.body.replaceChildren();
    const midLine = getComposerMenuAnchorRect({
      textarea: input(draft, 11 + 30),
      wide: false,
      menuWidth: 320,
    });
    expect(midLine.left).toBe(100 + 144);
  });

  it("spans the input inside the menu gutters on a phone", () => {
    const rect = getComposerMenuAnchorRect({
      textarea: input("draft", 5),
      wide: true,
      menuWidth: 320,
    });
    expect([rect.left, rect.top, rect.width]).toEqual([108, 500, 884]);
  });
});
