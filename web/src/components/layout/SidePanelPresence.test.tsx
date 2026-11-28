// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SidePanelPresence } from "./SidePanelPresence";

let host: HTMLDivElement;
let root: Root;
let animated: boolean;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  animated = true;
  // The DOM here loads no stylesheets: whether the current style animates the closing is
  // the one thing the browser would report.
  const real = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation((element: Element) => {
    const style = real(element);
    return element.hasAttribute("data-side-panel-closing")
      ? ({
          ...style,
          animationName: animated ? "preview-side-panel-out" : "none",
        } as CSSStyleDeclaration)
      : style;
  });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const render = (open: boolean) =>
  act(() =>
    root.render(
      <SidePanelPresence open={open}>
        <div id="group-side-panel" style={{ width: "420px" }}>
          files
        </div>
      </SidePanelPresence>,
    ),
  );
const panel = () => host.querySelector("#group-side-panel");
const closing = () => host.querySelector<HTMLElement>("[data-side-panel-closing]");

it("holds the panel's width while it closes, then lets go", () => {
  render(true);
  expect(panel()).not.toBeNull();
  expect(closing()).toBeNull();

  render(false);
  // The content is gone at once; a spacer of the same width collapses in its place.
  expect(panel()).toBeNull();
  expect(closing()?.style.width).toBe("420px");

  act(() => {
    closing()!.dispatchEvent(new Event("animationend", { bubbles: true }));
  });
  expect(closing()).toBeNull();
});

it("drops the space at once where nothing animates it", () => {
  animated = false;
  render(true);
  render(false);
  expect(panel()).toBeNull();
  expect(closing()).toBeNull();
});

it("shows a reopened panel without a leftover spacer", () => {
  render(true);
  render(false);
  expect(closing()).not.toBeNull();
  render(true);
  expect(panel()).not.toBeNull();
  expect(closing()).toBeNull();
});

it("renders no spacer for a panel that was never open", () => {
  render(false);
  expect(closing()).toBeNull();
});
