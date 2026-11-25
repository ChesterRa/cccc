// @vitest-environment happy-dom

import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vite-plus/test";

import { attachTerminalTouchScroll } from "./terminalTouchScroll";

function dispatchTouch(element: HTMLElement, type: string, clientY?: number, clientX = 120): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", {
    value: clientY === undefined ? [] : [{ clientY, clientX }],
  });
  element.dispatchEvent(event);
  return event;
}

function setupScrollableFollower(options: {
  clientWidth: number;
  scrollWidth: number;
  clientHeight: number;
  scrollHeight: number;
  overflow?: string;
  mouseTrackingMode?: string;
  bufferType?: "normal" | "alternate";
  baseY?: number;
  viewportY?: number;
  writable?: boolean;
}) {
  const container = document.createElement("div");
  container.style.overflow = options.overflow ?? "auto";
  Object.defineProperty(container, "clientWidth", {
    value: options.clientWidth,
    configurable: true,
  });
  Object.defineProperty(container, "scrollWidth", {
    value: options.scrollWidth,
    configurable: true,
  });
  Object.defineProperty(container, "clientHeight", {
    value: options.clientHeight,
    configurable: true,
  });
  Object.defineProperty(container, "scrollHeight", {
    value: options.scrollHeight,
    configurable: true,
  });
  let scrollLeft = 0;
  let scrollTop = 0;
  Object.defineProperty(container, "scrollLeft", {
    get: () => scrollLeft,
    set: (v: number) => {
      scrollLeft = v;
    },
    configurable: true,
  });
  Object.defineProperty(container, "scrollTop", {
    get: () => scrollTop,
    set: (v: number) => {
      scrollTop = v;
    },
    configurable: true,
  });

  const element = document.createElement("div");
  const screen = document.createElement("div");
  screen.className = "xterm-screen";
  screen.getBoundingClientRect = () =>
    ({
      width: options.scrollWidth,
      height: options.scrollHeight,
      top: 0,
      right: options.scrollWidth,
      bottom: options.scrollHeight,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
  element.appendChild(screen);
  container.appendChild(element);

  const baseY = options.baseY ?? 0;
  let viewportY = options.viewportY ?? baseY;
  const scrollLines = vi.fn((lines: number) => {
    viewportY = Math.max(0, Math.min(baseY, viewportY + lines));
  });
  const focus = vi.fn();
  const activeBuffer = {
    type: options.bufferType ?? "normal",
    get baseY() {
      return baseY;
    },
    get viewportY() {
      return viewportY;
    },
  };
  const term = {
    element,
    rows: Math.max(1, Math.round(options.scrollHeight / 18)),
    scrollLines,
    focus,
    modes: { mouseTrackingMode: options.mouseTrackingMode ?? "none" },
    buffer: { active: activeBuffer },
    options: { disableStdin: false },
  } as unknown as Terminal;

  let writable = options.writable ?? false;
  const elementWheels: WheelEvent[] = [];
  element.addEventListener("wheel", (event) => elementWheels.push(event));
  const dispose = attachTerminalTouchScroll(term, () => writable);
  return {
    container,
    element,
    screen,
    scrollLines,
    focus,
    dispose,
    elementWheels,
    term,
    getViewportY: () => viewportY,
    setWritable: (value: boolean) => {
      writable = value;
    },
  };
}

function setupTerminal(mouseTrackingMode = "none", bufferType = "normal", writable = true) {
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
    options: { disableStdin: false },
  } as unknown as Terminal;
  const wheels: WheelEvent[] = [];
  element.addEventListener("wheel", (event) => wheels.push(event));
  const dispose = attachTerminalTouchScroll(term, () => writable);
  return {
    element,
    scrollLines,
    focus,
    dispose,
    wheels,
    term,
    setWritable: (value: boolean) => {
      writable = value;
    },
  };
}

describe("terminal touch scroll", () => {
  it.each(["vt200", "drag", "any"])(
    "keeps local history usable across writer handoffs in %s mode",
    (mode) => {
      const { element, scrollLines, wheels, setWritable, term } = setupTerminal(
        mode,
        "normal",
        false,
      );
      dispatchTouch(element, "touchstart", 100);
      dispatchTouch(element, "touchmove", 154);
      expect(scrollLines.mock.calls).toEqual([[-3]]);
      expect(wheels).toHaveLength(0);
      setWritable(true);
      dispatchTouch(element, "touchmove", 208);
      expect(wheels).toHaveLength(3);
      expect(scrollLines.mock.calls).toEqual([[-3]]);
      setWritable(false);
      dispatchTouch(element, "touchmove", 262);
      expect(scrollLines.mock.calls).toEqual([[-3], [-3]]);
      expect(wheels).toHaveLength(3);
      setWritable(true);
      term.options.disableStdin = true;
      dispatchTouch(element, "touchmove", 316);
      expect(scrollLines.mock.calls).toEqual([[-3], [-3], [-3]]);
      expect(wheels).toHaveLength(3);
    },
  );

  it("does not synthesize application input for a read-only alternate buffer", () => {
    const { element, wheels } = setupTerminal("vt200", "alternate", false);
    dispatchTouch(element, "touchstart", 100);
    dispatchTouch(element, "touchmove", 154);
    expect(wheels).toHaveLength(0);
  });
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

  it("pans a narrow read-only follower horizontally without triggering focus or vertical jitter", () => {
    const { container, element, scrollLines, focus } = setupScrollableFollower({
      clientWidth: 390,
      scrollWidth: 1017,
      clientHeight: 691,
      scrollHeight: 540,
      baseY: 50,
    });

    dispatchTouch(element, "touchstart", 200, 320);
    dispatchTouch(element, "touchmove", 185, 120);
    dispatchTouch(element, "touchmove", 170, -400);
    dispatchTouch(element, "touchend");

    expect(container.scrollLeft).toBe(627);
    expect(scrollLines).not.toHaveBeenCalled();
    expect(focus).not.toHaveBeenCalled();
  });

  it("pans both axes in an overflowing read-only alternate-buffer follower without sending wheel input", () => {
    const { container, element, scrollLines, elementWheels } = setupScrollableFollower({
      clientWidth: 572,
      scrollWidth: 1017,
      clientHeight: 300,
      scrollHeight: 540,
      bufferType: "alternate",
      mouseTrackingMode: "vt200",
    });

    dispatchTouch(element, "touchstart", 240, 300);
    dispatchTouch(element, "touchmove", 60, 40);
    dispatchTouch(element, "touchmove", -40, -260);
    dispatchTouch(element, "touchend");

    expect(container.scrollLeft).toBe(445);
    expect(container.scrollTop).toBe(240);
    expect(scrollLines).not.toHaveBeenCalled();
    expect(elementWheels).toHaveLength(0);
  });

  it("bridges normal-buffer scrollback and vertical container overflow seamlessly on touch", () => {
    const { container, element, scrollLines, getViewportY } = setupScrollableFollower({
      clientWidth: 572,
      scrollWidth: 1017,
      clientHeight: 300,
      scrollHeight: 540,
      bufferType: "normal",
      baseY: 10,
      viewportY: 10,
    });

    // At live bottom: swiping up pans container down to reveal bottom rows.
    dispatchTouch(element, "touchstart", 250, 200);
    dispatchTouch(element, "touchmove", 150, 200);
    expect(container.scrollTop).toBe(100);
    expect(scrollLines).not.toHaveBeenCalled();

    // Swiping down unwinds container scrollTop first, then scrolls up into scrollback.
    dispatchTouch(element, "touchmove", 286, 200);
    expect(container.scrollTop).toBe(0);
    expect(scrollLines).toHaveBeenCalledWith(-2);
    expect(getViewportY()).toBe(8);

    // Swiping up again returns to live bottom first, then pans container down with leftover pixels.
    dispatchTouch(element, "touchmove", 196, 200);
    expect(getViewportY()).toBe(10);
    expect(container.scrollTop).toBe(54);
  });

  it("stops wheel propagation before xterm cancels it when a read-only follower overflows", () => {
    const { container, screen, elementWheels, dispose, setWritable } = setupScrollableFollower({
      clientWidth: 572,
      scrollWidth: 1017,
      clientHeight: 300,
      scrollHeight: 540,
      bufferType: "alternate",
      mouseTrackingMode: "vt200",
    });

    // Vertical and horizontal wheel in read-only alternate/mouse-tracking mode bypass xterm.
    screen.dispatchEvent(new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true }));
    screen.dispatchEvent(new WheelEvent("wheel", { deltaX: 80, bubbles: true, cancelable: true }));
    screen.dispatchEvent(
      new WheelEvent("wheel", { deltaY: 120, shiftKey: true, bubbles: true, cancelable: true }),
    );
    expect(elementWheels).toHaveLength(0);

    // Once writable, wheel events reach xterm normally.
    setWritable(true);
    screen.dispatchEvent(new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true }));
    expect(elementWheels).toHaveLength(1);

    // After cleanup, capture listener is removed.
    setWritable(false);
    dispose();
    screen.dispatchEvent(new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true }));
    expect(elementWheels).toHaveLength(2);
    expect(container.scrollTop).toBe(0);
  });

  it("keeps normal-buffer wheel scrollback working while letting overflow edges scroll the container", () => {
    const { container, screen, elementWheels } = setupScrollableFollower({
      clientWidth: 572,
      scrollWidth: 1017,
      clientHeight: 300,
      scrollHeight: 540,
      bufferType: "normal",
      baseY: 20,
      viewportY: 20,
    });

    // At live bottom (viewportY === baseY): wheeling down bypasses xterm to scroll container.
    screen.dispatchEvent(new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true }));
    expect(elementWheels).toHaveLength(0);

    // At scrollTop === 0: wheeling up reaches xterm to scroll up into history.
    screen.dispatchEvent(
      new WheelEvent("wheel", { deltaY: -120, bubbles: true, cancelable: true }),
    );
    expect(elementWheels).toHaveLength(1);

    // When scrolled down in container (scrollTop > 0): wheeling up unwinds container first.
    container.scrollTop = 120;
    screen.dispatchEvent(
      new WheelEvent("wheel", { deltaY: -120, bubbles: true, cancelable: true }),
    );
    expect(elementWheels).toHaveLength(1);
  });

  it("ignores container overflow when container overflow is hidden", () => {
    const { container, element, screen, elementWheels, scrollLines } = setupScrollableFollower({
      clientWidth: 390,
      scrollWidth: 1017,
      clientHeight: 300,
      scrollHeight: 540,
      overflow: "hidden",
    });

    dispatchTouch(element, "touchstart", 200, 300);
    dispatchTouch(element, "touchmove", 128, 100);
    expect(container.scrollLeft).toBe(0);
    expect(container.scrollTop).toBe(0);
    expect(scrollLines).toHaveBeenCalledWith(4);

    screen.dispatchEvent(new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true }));
    expect(elementWheels).toHaveLength(1);
  });
});
