// @vitest-environment happy-dom
import { act, useRef, type RefObject } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { useMarginWheel } from "./useMarginWheel";
import { useReadingEngagement } from "./useReadingEngagement";

let host: HTMLDivElement;
let root: Root;
let engagement: ReturnType<typeof useReadingEngagement>;
let logWheels: number[];

// The list's area: a margin and a floating control beside the log, as on a wide screen.
function Area() {
  const ref = useRef<HTMLDivElement>(null);
  engagement = useReadingEngagement(ref as RefObject<HTMLElement | null>);
  useMarginWheel(ref as RefObject<HTMLElement | null>);
  return (
    <div data-area>
      <div data-margin />
      <div ref={ref} data-log onWheel={(event) => logWheels.push(event.deltaY)}>
        <div data-message-row="true" data-message-id="m1">
          <div data-nested-scroller />
        </div>
      </div>
      <button type="button" data-floating>
        latest
      </button>
    </div>
  );
}

function wheel(target: Element, init: WheelEventInit) {
  const event = new WheelEvent("wheel", { bubbles: true, cancelable: true, ...init });
  // happy-dom drops modifier keys from a wheel's init; a browser keeps them.
  for (const key of ["ctrlKey", "shiftKey"] as const)
    if (init[key]) Object.defineProperty(event, key, { value: true });
  target.dispatchEvent(event);
  return event;
}

const el = (selector: string) => host.querySelector<HTMLElement>(selector)!;

beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  logWheels = [];
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Area />));
  Object.defineProperty(el("[data-log]"), "clientHeight", { configurable: true, value: 500 });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("useMarginWheel", () => {
  it("scrolls the log by a wheel over the margin, counted as the reader's own scroll", () => {
    const log = el("[data-log]");
    const intent = engagement.scrollIntentIdRef.current;
    wheel(el("[data-margin]"), { deltaY: 120 });
    expect(log.scrollTop).toBe(120);
    expect(logWheels).toEqual([120]);
    expect(engagement.scrollIntentIdRef.current).toBe(intent + 1);
    expect(engagement.engagedRef.current).toBe(true);

    wheel(el("[data-floating]"), { deltaY: -50 });
    expect(log.scrollTop).toBe(70);
    expect(logWheels).toEqual([120, -50]);
  });

  it("moves by lines and pages as the wheel reports them", () => {
    const log = el("[data-log]");
    wheel(el("[data-margin]"), { deltaY: 3, deltaMode: 1 });
    expect(log.scrollTop).toBe(60);
    wheel(el("[data-margin]"), { deltaY: 1, deltaMode: 2 });
    expect(log.scrollTop).toBe(560);
  });

  it("leaves a wheel on the log, or inside it, to the log alone", () => {
    const log = el("[data-log]");
    wheel(log, { deltaY: 100 });
    wheel(el("[data-nested-scroller]"), { deltaY: 100 });
    expect(log.scrollTop).toBe(0);
    expect(logWheels).toEqual([100, 100]);
  });

  it("leaves zoom, sideways, Shift and handled wheels, and wheels elsewhere, to the browser", () => {
    const log = el("[data-log]");
    const margin = el("[data-margin]");
    const moved = (init: WheelEventInit) => {
      const before = log.scrollTop;
      const event = wheel(margin, init);
      expect(event.deltaY).toBe(init.deltaY);
      return log.scrollTop - before;
    };
    expect(moved({ deltaY: 100, ctrlKey: true })).toBe(0);
    expect(moved({ deltaY: 100, shiftKey: true })).toBe(0);
    expect(moved({ deltaX: 100, deltaY: 20 })).toBe(0);
    margin.addEventListener("wheel", (event) => event.preventDefault(), { once: true });
    expect(moved({ deltaY: 100 })).toBe(0);
    const menu = document.createElement("div");
    document.body.append(menu);
    wheel(menu, { deltaY: 100 });
    menu.remove();
    expect(log.scrollTop).toBe(0);
    expect(logWheels).toEqual([]);
  });
});
