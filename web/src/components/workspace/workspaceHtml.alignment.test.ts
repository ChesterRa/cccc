// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { preserveStaticHtmlAlignment } from "./workspaceHtml";

let doc: Document;
let frame: HTMLIFrameElement;
let column: HTMLDivElement;
let child: HTMLDivElement;
let frameDescriptor: PropertyDescriptor | undefined;
let cleanup: (() => void) | undefined;
const disconnect = vi.fn();

function box(element: Element, left: number, width: number) {
  vi.spyOn(element, "getBoundingClientRect").mockReturnValue(new DOMRect(left, 0, width, 100));
}

beforeEach(() => {
  doc = document.implementation.createHTMLDocument();
  Object.defineProperty(doc, "defaultView", { value: window });
  doc.documentElement.style.direction = "ltr";
  frame = document.createElement("iframe");
  Object.defineProperty(frame, "contentDocument", { value: doc });
  frameDescriptor = Object.getOwnPropertyDescriptor(window, "frameElement");
  Object.defineProperty(window, "frameElement", { configurable: true, value: frame });
  column = doc.createElement("div");
  column.style.cssText =
    "display:flex;flex-direction:column;writing-mode:horizontal-tb;direction:ltr;overflow:visible";
  column.style.setProperty("align-items", "center", "important");
  child = doc.createElement("div");
  column.append(child);
  doc.body.append(column);
  box(column, 0, 390);
  box(child, -445, 1280);
  disconnect.mockClear();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect = disconnect;
    },
  );
});

afterEach(() => {
  cleanup?.();
  cleanup = undefined;
  if (frameDescriptor) Object.defineProperty(window, "frameElement", frameDescriptor);
  else Reflect.deleteProperty(window, "frameElement");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("makes a column item before the document start reachable and restores the author's priority", () => {
  cleanup = preserveStaticHtmlAlignment(doc);
  expect(column.style.alignItems).toBe("safe center");
  cleanup();
  expect(column.style.alignItems).toBe("center");
  expect(column.style.getPropertyPriority("align-items")).toBe("important");
  expect(disconnect).toHaveBeenCalledOnce();
  cleanup = undefined;
});

it("preserves an explicitly clipped ancestor's centred image", () => {
  const mask = doc.createElement("div");
  mask.style.overflowX = "hidden";
  column.replaceWith(mask);
  mask.append(column);
  cleanup = preserveStaticHtmlAlignment(doc);
  expect(column.style.alignItems).toBe("center");
});

it("does not move an item that overflows its local column but is already fully visible", () => {
  box(column, 150, 200);
  box(child, 100, 300);
  cleanup = preserveStaticHtmlAlignment(doc);
  expect(column.style.alignItems).toBe("center");
});

it("uses a nested auto scroller's start rather than the document's start", () => {
  column.style.overflowX = "auto";
  column.style.overflowY = "auto";
  box(column, 150, 200);
  box(child, 100, 300);
  cleanup = preserveStaticHtmlAlignment(doc);
  expect(column.style.alignItems).toBe("safe center");
});
