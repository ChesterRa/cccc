// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { useModalA11y } from "./useModalA11y";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
afterEach(() => {
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

function Fixture() {
  const { modalRef } = useModalA11y(true, () => {});
  return (
    <div ref={modalRef}>
      <div hidden>
        <button>hidden first</button>
      </div>
      <button id="first">first</button>
      <details>
        <summary tabIndex={0}>advanced</summary>
        <button>closed detail</button>
      </details>
      <button id="last">last</button>
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
