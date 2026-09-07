// @vitest-environment happy-dom

import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vite-plus/test";

import { attachTerminalTouchScroll } from "./terminalTouchScroll";

function dispatchTouch(element: HTMLElement, type: string, clientY?: number): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", {
    value: clientY === undefined ? [] : [{ clientY, clientX: 120 }],
  });
  element.dispatchEvent(event);
  return event;
}

function setupTerminal(mouseTrackingMode = "none", bufferType = "normal") {
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
  const term = {
    element,
    rows: 20,
    scrollLines,
    focus,
    modes: { mouseTrackingMode },
    buffer: { active: { type: bufferType } },
  } as unknown as Terminal;
  const wheels: WheelEvent[] = [];
  element.addEventListener("wheel", (event) => wheels.push(event));
  const dispose = attachTerminalTouchScroll(term);
  return { element, scrollLines, focus, dispose, wheels, term };
}

describe("terminal touch scroll", () => {
  it.each(["none", "x10"])("scrolls local history in normal-buffer %s mode", (mode) => {
    const { element, scrollLines, focus, wheels } = setupTerminal(mode);

    dispatchTouch(element, "touchstart", 100);
    const move = dispatchTouch(element, "touchmove", 64);
    dispatchTouch(element, "touchend");

    expect(move.defaultPrevented).toBe(true);
    expect(scrollLines).toHaveBeenCalledWith(2);
    expect(wheels).toHaveLength(0);
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

  it.each([
    ["vt200", "normal"],
    ["drag", "normal"],
    ["any", "normal"],
    ["none", "alternate"],
    ["x10", "alternate"],
  ])("dispatches one xterm wheel event per accumulated line for %s/%s", (mouse, buffer) => {
    const { element, scrollLines, wheels, dispose } = setupTerminal(mouse, buffer);
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchmove", 110);
    expect(wheels).toHaveLength(0);
    dispatchTouch(element, "touchmove", 154);
    expect(wheels).toHaveLength(3);
    expect(wheels.map((event) => event.deltaY)).toEqual([-1, -1, -1]);
    for (const event of wheels) {
      expect(event.deltaMode).toBe(WheelEvent.DOM_DELTA_LINE);
      expect(event.bubbles).toBe(true);
      expect(event.cancelable).toBe(true);
      expect(event.target).toBe(element);
      // happy-dom WheelEvent lacks MouseEvent coordinates; browser QA covers xterm encoding.
    }
    dispatchTouch(element, "touchmove", 118);
    expect(wheels.map((event) => event.deltaY)).toEqual([-1, -1, -1, 1, 1]);
    expect(scrollLines).not.toHaveBeenCalled();
    dispose();
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchmove", 136);
    expect(wheels).toHaveLength(5);
  });

  it("switches back to local scrolling when a gesture enters X10 mode", () => {
    const { element, term, scrollLines, wheels } = setupTerminal("vt200");
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchmove", 64);
    expect(wheels).toHaveLength(2);
    expect(scrollLines).not.toHaveBeenCalled();

    // The mock exposes the same live mode reads as xterm; browser QA below
    // verifies that real DECSET sequences update this property.
    Object.assign(term.modes, { mouseTrackingMode: "x10" });
    dispatchTouch(element, "touchmove", 28);
    expect(scrollLines).toHaveBeenCalledWith(2);
    expect(wheels).toHaveLength(2);
  });

  it("keeps taps as focus-only in mouse tracking mode", () => {
    const { element, focus, scrollLines, wheels } = setupTerminal("any");
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchend");
    expect(focus).toHaveBeenCalledOnce();
    expect(wheels).toHaveLength(0);
    expect(scrollLines).not.toHaveBeenCalled();
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
