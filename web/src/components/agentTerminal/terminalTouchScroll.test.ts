// @vitest-environment happy-dom

import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vite-plus/test";

import { attachTerminalTouchScroll } from "./terminalTouchScroll";

function dispatchTouch(element: HTMLElement, type: string, clientY?: number): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", { value: clientY === undefined ? [] : [{ clientY }] });
  element.dispatchEvent(event);
  return event;
}

function setupTerminal() {
  const element = document.createElement("div");
  const screen = document.createElement("div");
  screen.className = "xterm-screen";
  screen.getBoundingClientRect = () =>
    ({
      width: 320,
      height: 360,
      top: 0,
      right: 320,
      bottom: 360,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
  element.appendChild(screen);

  const scrollLines = vi.fn();
  const focus = vi.fn();
  const term = { element, rows: 20, scrollLines, focus } as unknown as Terminal;
  const dispose = attachTerminalTouchScroll(term);
  return { element, scrollLines, focus, dispose };
}

describe("terminal touch scroll", () => {
  it("converts vertical touch movement into xterm scrollback lines", () => {
    const { element, scrollLines, focus } = setupTerminal();

    dispatchTouch(element, "touchstart", 100);
    const move = dispatchTouch(element, "touchmove", 64);
    dispatchTouch(element, "touchend");

    expect(move.defaultPrevented).toBe(true);
    expect(scrollLines).toHaveBeenCalledWith(2);
    expect(focus).not.toHaveBeenCalled();
  });

  it("accumulates sub-line movement and preserves scroll direction", () => {
    const { element, scrollLines } = setupTerminal();

    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchmove", 90);
    dispatchTouch(element, "touchmove", 80);
    dispatchTouch(element, "touchmove", 100);

    expect(scrollLines.mock.calls).toEqual([[1], [-1]]);
  });

  it("focuses the terminal only for a tap and restores styles on cleanup", () => {
    const { element, scrollLines, focus, dispose } = setupTerminal();

    expect(element.style.touchAction).toBe("none");
    expect(element.style.overscrollBehavior).toBe("contain");
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchend");

    expect(focus).toHaveBeenCalledOnce();
    expect(scrollLines).not.toHaveBeenCalled();

    dispose();
    expect(element.style.touchAction).toBe("");
    expect(element.style.overscrollBehavior).toBe("");
  });
});
