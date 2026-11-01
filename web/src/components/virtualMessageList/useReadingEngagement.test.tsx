// @vitest-environment happy-dom
import { act, useRef, type RefObject } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { getReadingFocus, useReadingEngagement } from "./useReadingEngagement";

let host: HTMLDivElement;
let root: Root;
let engagement: ReturnType<typeof useReadingEngagement>;

function Log() {
  const ref = useRef<HTMLDivElement>(null);
  engagement = useReadingEngagement(ref as RefObject<HTMLElement | null>);
  return (
    <div ref={ref} data-log>
      <div data-message-row="true" data-message-id="m1">
        <button type="button" data-in-row>
          act
        </button>
      </div>
    </div>
  );
}

beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Log />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("useReadingEngagement", () => {
  it("counts scrolling and touching a message, and remembers which message", () => {
    const log = host.querySelector<HTMLElement>("[data-log]")!;
    expect(engagement.engagedRef.current).toBe(false);
    log.dispatchEvent(new Event("wheel"));
    expect(engagement.engagedRef.current).toBe(true);
    expect(engagement.touchedIdRef.current).toBe("");
    host.querySelector("[data-in-row]")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(engagement.touchedIdRef.current).toBe("m1");
  });

  it("keeps the touched message until the reader scrolls the list itself", () => {
    const log = host.querySelector<HTMLElement>("[data-log]")!;
    host.querySelector("[data-in-row]")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    // A restore or auto-follow scrolls without a wheel, and a wheel in a message's own
    // scroller moves nothing here: both keep it.
    log.dispatchEvent(new Event("scroll"));
    log.dispatchEvent(new Event("wheel"));
    expect(engagement.touchedIdRef.current).toBe("m1");
    log.dispatchEvent(new Event("scroll"));
    expect(engagement.touchedIdRef.current).toBe("");
  });

  it("lets a scroll key clear the touch only when the list then scrolls", () => {
    const log = host.querySelector<HTMLElement>("[data-log]")!;
    const button = host.querySelector("[data-in-row]")!;
    // Space on a message's button activates it: the message stays touched.
    button.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(engagement.touchedIdRef.current).toBe("m1");
    // PageDown from that button scrolls the list: the middle message takes over.
    button.dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown", bubbles: true }));
    log.dispatchEvent(new Event("scroll"));
    expect(engagement.touchedIdRef.current).toBe("");
  });

  it("never treats keys typed in a field as scrolling", () => {
    const log = host.querySelector<HTMLElement>("[data-log]")!;
    const field = document.createElement("textarea");
    host.querySelector('[data-message-row="true"]')!.append(field);
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    log.dispatchEvent(new Event("scroll"));
    expect(engagement.touchedIdRef.current).toBe("m1");
  });

  it("ignores programmatic focus but counts focus reached with Tab", () => {
    const button = host.querySelector<HTMLButtonElement>("[data-in-row]")!;
    button.focus();
    expect(engagement.engagedRef.current).toBe(false);
    button.blur();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab" }));
    button.focus();
    expect(engagement.engagedRef.current).toBe(true);
    expect(engagement.touchedIdRef.current).toBe("m1");
  });
});

describe("getReadingFocus", () => {
  const rect = (top: number, bottom: number) => ({ top, bottom }) as DOMRect;
  function list(rows: Array<[string, number, number]>) {
    const container = document.createElement("div");
    container.getBoundingClientRect = () => rect(0, 400);
    for (const [id, top, bottom] of rows) {
      const row = document.createElement("div");
      row.dataset.messageRow = "true";
      row.dataset.messageId = id;
      row.getBoundingClientRect = () => rect(top, bottom);
      container.append(row);
    }
    return container;
  }
  const always = () => true;

  it("prefers the message last touched while it is on screen, else the one across the middle", () => {
    const container = list([
      ["a", -50, 100],
      ["b", 100, 260],
      ["c", 260, 420],
    ]);
    // Even partly hidden: a message just clicked at the edge is the one being read.
    expect(getReadingFocus(container, 0, "c", always)).toEqual({
      focusId: "c",
      focusOffsetPx: -260,
      visible: [
        { id: "c", offsetPx: -260 },
        { id: "b", offsetPx: -100 },
        { id: "a", offsetPx: 50 },
      ],
      touched: true,
    });
    expect(getReadingFocus(container, 0, "gone", always)).toEqual({
      focusId: "b",
      focusOffsetPx: -100,
      visible: [
        { id: "b", offsetPx: -100 },
        { id: "c", offsetPx: -260 },
        { id: "a", offsetPx: 50 },
      ],
      touched: false,
    });
  });

  it("orders what is on screen by how near its shown part is to the middle", () => {
    // The middle is at 200 and b crosses it. a began far above the top, but what is left of it
    // ends 50px above the middle: nearer than c, which starts 60px below.
    const container = list([
      ["a", -600, 150],
      ["b", 150, 260],
      ["c", 260, 300],
    ]);
    expect(getReadingFocus(container, 0, "", always)?.visible.map((row) => row.id)).toEqual([
      "b",
      "a",
      "c",
    ]);
  });

  it("skips rows it may not use and rows outside the reading area", () => {
    const container = list([
      ["above", -200, -10],
      ["pending", 150, 250],
      ["c", 260, 420],
    ]);
    expect(getReadingFocus(container, 0, "", (id) => id !== "pending")?.focusId).toBe("c");
    expect(getReadingFocus(list([["above", -200, -10]]), 0, "", always)).toBeNull();
  });
});
