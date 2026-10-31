// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useTaskKeysPlacement } from "./useTaskKeysPlacement";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let layout: { toolbarBottom: number; tops: Record<string, number> };
let host: HTMLDivElement;
let root: Root;
let observers: Array<{ callback: () => void; targets: Element[] }>;

beforeEach(() => {
  // The scroller starts at 100 and the toolbar pinned to it ends at 160.
  layout = { toolbarBottom: 160, tops: { a: 162, b: 300 } };
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    if (this.hasAttribute("data-context-scroll")) return new DOMRect(0, 100, 800, 600);
    if (this.hasAttribute("data-taskboard-sticky"))
      return new DOMRect(0, layout.toolbarBottom - 60, 800, 60);
    const id = this.getAttribute("data-testid");
    return id ? new DOMRect(0, layout.tops[id], 800, 70) : new DOMRect();
  });
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.hasAttribute("data-task-actions") ? 18 : 0;
    },
  );
  observers = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(callback: () => void) {
        observers.push({ callback, targets: this.targets });
      }
      observe(target: Element) {
        this.targets.push(target);
      }
      disconnect() {
        this.targets.length = 0;
      }
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Fixture />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function Fixture() {
  const boardRef = useRef<HTMLDivElement>(null);
  useTaskKeysPlacement(boardRef, true);
  return (
    <div data-context-scroll="">
      <section data-taskboard="">
        <div data-taskboard-sticky="" />
        <div ref={boardRef}>
          {["a", "b"].map((id) => (
            <div key={id} data-task-row="" data-testid={id} style={{ paddingTop: "8px" }}>
              <button type="button" data-task-title="">
                {id}
              </button>
              <span data-task-actions="" />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

const row = (id: string) => host.querySelector<HTMLElement>(`[data-testid="${id}"]`)!;
const raised = (id: string) => row(id).hasAttribute("data-task-keys-raised");
const send = (id: string, type: "pointerover" | "focusin") =>
  row(id)
    .querySelector("[data-task-title]")!
    .dispatchEvent(new Event(type, { bubbles: true }));
const scroll = () =>
  host.querySelector("[data-context-scroll]")!.dispatchEvent(new Event("scroll"));

it("raises the keys of a hovered row while its text is in view under the toolbar", () => {
  send("a", "pointerover");
  expect(raised("a")).toBe(true);

  // Its rule 6px under the toolbar, its text still below it.
  layout.tops.a = 154;
  send("a", "pointerover");
  expect(raised("a")).toBe(true);

  // Its text has reached the toolbar: the keys stay under with it.
  layout.tops.a = 150;
  send("a", "pointerover");
  expect(raised("a")).toBe(false);

  // Far enough down, the keys clear the toolbar on their own.
  layout.tops.a = 170;
  send("a", "pointerover");
  expect(raised("a")).toBe(false);
});

it("places the hovered and the focused row again when the board scrolls or the toolbar grows", () => {
  send("a", "pointerover");
  send("b", "focusin");
  expect([raised("a"), raised("b")]).toEqual([true, false]);

  layout.tops = { a: 90, b: 163 };
  scroll();
  expect([raised("a"), raised("b")]).toEqual([false, true]);

  // A status line pins under the toolbar, which now ends lower.
  layout.toolbarBottom = 190;
  scroll();
  expect(raised("b")).toBe(false);
});

it("places the focused row again when the layout moves it without a scroll", () => {
  layout.tops.b = 198;
  send("b", "focusin");
  expect(raised("b")).toBe(false);

  // A wider window rewraps the rows above, and b reaches the toolbar. The board
  // section changes size, which is what the browser reports.
  layout.tops.b = 162;
  for (const { callback, targets } of observers)
    if (targets.some((target) => target.hasAttribute("data-taskboard"))) callback();
  expect(raised("b")).toBe(true);
});

it("places the focused row again when the board's text changes above it", async () => {
  layout.tops.b = 198;
  send("b", "focusin");
  expect(raised("b")).toBe(false);

  // The minute clock shortens a line in row a; no box in the column changes size.
  layout.tops.b = 162;
  row("a").querySelector("[data-task-title]")!.firstChild!.nodeValue = "a, an hour ago";
  await act(async () => {});
  expect(raised("b")).toBe(true);
});
