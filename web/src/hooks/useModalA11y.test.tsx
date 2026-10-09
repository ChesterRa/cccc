// @vitest-environment happy-dom
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../components/ui/dialog";
import { useModalA11y } from "./useModalA11y";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

function Fixture() {
  const { modalRef } = useModalA11y(true, () => {});
  return (
    <div ref={modalRef}>
      <div hidden>
        <button>hidden first</button>
      </div>
      <fieldset disabled>
        <button>disabled first</button>
      </fieldset>
      <button id="first">first</button>
      <details>
        <summary tabIndex={0}>advanced</summary>
        <button>closed detail</button>
      </details>
      <button id="last">last</button>
      <fieldset disabled>
        <button>disabled last</button>
      </fieldset>
      <div inert>
        <button>inert last</button>
      </div>
      <div aria-hidden="true">
        <button>aria hidden last</button>
      </div>
      <div style={{ display: "none" }}>
        <button>CSS hidden last</button>
      </div>
      <div style={{ visibility: "hidden" }}>
        <button>invisible last</button>
      </div>
    </div>
  );
}

describe("modal keyboard focus", () => {
  it("starts and wraps on visible controls, skipping hidden panels, inert content and closed details", async () => {
    // happy-dom does not yet implement fieldset-inherited :disabled. Model
    // that browser CSS state here; the native browser acceptance also exercises it.
    const matches = HTMLElement.prototype.matches;
    vi.spyOn(HTMLElement.prototype, "matches").mockImplementation(function (
      this: HTMLElement,
      selector: string,
    ) {
      if (selector === ":disabled" && this.closest("fieldset[disabled]")) return true;
      return matches.call(this, selector);
    });
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => root.render(<Fixture />));
    const first = host.querySelector<HTMLButtonElement>("#first")!;
    const last = host.querySelector<HTMLButtonElement>("#last")!;
    expect(document.activeElement).toBe(first);
    const backwards = new KeyboardEvent("keydown", {
      key: "Tab",
      shiftKey: true,
      cancelable: true,
    });
    document.dispatchEvent(backwards);
    expect(backwards.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(last);
    const forwards = new KeyboardEvent("keydown", { key: "Tab", cancelable: true });
    document.dispatchEvent(forwards);
    expect(forwards.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(first);
    // Removing the last visible control makes the summary the new boundary.
    last.hidden = true;
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true }));
    expect(document.activeElement).toBe(host.querySelector("summary"));
    await act(async () => root.unmount());
    expect(document.body.style.overflow).toBe("");
  });
});

it("honors an initial field and lets a nested popup consume Escape before the modal", async () => {
  const close = vi.fn();
  function SearchFixture() {
    const input = useRef<HTMLInputElement>(null);
    const { modalRef } = useModalA11y(true, close, { initialFocusRef: input });
    return (
      <div ref={modalRef}>
        <button>Close</button>
        <input ref={input} />
      </div>
    );
  }
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<SearchFixture />));
  expect(document.activeElement).toBe(host.querySelector("input"));
  const nestedEscape = (event: KeyboardEvent) => event.preventDefault();
  document.addEventListener("keydown", nestedEscape, { capture: true });
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
  expect(close).not.toHaveBeenCalled();
  document.removeEventListener("keydown", nestedEscape, { capture: true });
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
  expect(close).toHaveBeenCalledOnce();
  await act(async () => root.unmount());
});

it("leaves focus inside a portaled child dialog and restores the parent trigger after Escape", async () => {
  const close = vi.fn();
  function NestedFixture() {
    const [childOpen, setChildOpen] = useState(false);
    const { modalRef } = useModalA11y(true, close);
    return (
      <div ref={modalRef}>
        <button id="parent-first">Parent first</button>
        <button id="open-child" onClick={() => setChildOpen(true)}>
          New folder
        </button>
        <Dialog open={childOpen} onOpenChange={setChildOpen}>
          <DialogContent>
            <DialogTitle>New folder</DialogTitle>
            <DialogDescription>Folder name</DialogDescription>
            <input id="child-name" />
            <button id="child-save">Save</button>
          </DialogContent>
        </Dialog>
      </div>
    );
  }
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<NestedFixture />));
  const trigger = host.querySelector<HTMLButtonElement>("#open-child")!;
  trigger.focus();
  await act(async () => trigger.click());
  const child = document.querySelector<HTMLElement>('[role="dialog"]')!;
  const childInput = document.querySelector<HTMLInputElement>("#child-name")!;
  expect(host.contains(child)).toBe(false);
  expect(document.activeElement).toBe(childInput);
  childInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
  childInput.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true }),
  );
  expect(child.contains(document.activeElement)).toBe(true);
  await act(async () => {
    childInput.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  });
  // Radix restores focus from a timer scheduled during the unmount commit.
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(close).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(trigger);
  await act(async () => root.unmount());
});
