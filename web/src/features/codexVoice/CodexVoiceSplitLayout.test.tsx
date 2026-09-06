// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { CodexVoiceSplitLayout } from "./CodexVoiceSplitLayout";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
const storageKey = "cccc.codexVoice.splitRatio.v1";
let root: Root;
let host: HTMLDivElement;
let resize: () => void;
let width: number;
let captured: number | undefined;

beforeEach(() => {
  localStorage.clear();
  width = 1008;
  captured = undefined;
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({
    x: 20,
    y: 0,
    left: 20,
    top: 0,
    right: 20 + width,
    bottom: 600,
    width,
    height: 600,
    toJSON: () => ({}),
  }));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  Object.defineProperties(HTMLElement.prototype, {
    setPointerCapture: {
      configurable: true,
      value: (id: number) => {
        captured = id;
      },
    },
    releasePointerCapture: {
      configurable: true,
      value: () => {
        captured = undefined;
      },
    },
    hasPointerCapture: { configurable: true, value: (id: number) => captured === id },
  });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  localStorage.clear();
  for (const key of ["setPointerCapture", "releasePointerCapture", "hasPointerCapture"]) {
    Reflect.deleteProperty(HTMLElement.prototype, key);
  }
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function render(enabled = true, active = true) {
  await act(async () =>
    root.render(
      <CodexVoiceSplitLayout
        enabled={enabled}
        active={active}
        conversation={<section>conversation</section>}
        analyst={<section data-terminal="true">terminal</section>}
      />,
    ),
  );
  return host.querySelector('[role="separator"]') as HTMLDivElement;
}
async function pointer(
  handle: HTMLDivElement,
  type: string,
  clientX: number,
  pointerType = "mouse",
) {
  await act(async () =>
    handle.dispatchEvent(
      new PointerEvent(type, { bubbles: true, pointerId: 1, button: 0, clientX, pointerType }),
    ),
  );
}
async function key(handle: HTMLDivElement, name: string) {
  await act(async () =>
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true })),
  );
}
function percent(handle: HTMLDivElement) {
  return Number(handle.getAttribute("aria-valuenow"));
}

describe("Voice pane resizing", () => {
  it("drags within readable pane widths, commits on release and restores after remount", async () => {
    const handle = await render();
    const terminal = host.querySelector("[data-terminal]");
    await pointer(handle, "pointerdown", 550);
    expect(captured).toBe(1);
    expect(document.activeElement).toBe(handle);
    await pointer(handle, "pointermove", 10000);
    expect(percent(handle)).toBe(64);
    expect(localStorage.getItem(storageKey)).toBeNull();
    await pointer(handle, "pointermove", -100);
    expect(percent(handle)).toBe(30);
    await pointer(handle, "pointerup", -100);
    expect(captured).toBeUndefined();
    expect(Number(localStorage.getItem(storageKey))).toBe(30);
    expect(host.querySelector("[data-terminal]")).toBe(terminal);
    await act(async () => root.unmount());
    root = createRoot(host);
    expect(percent(await render())).toBe(30);
  });

  it("supports keyboard adjustments and reset without requiring storage", async () => {
    localStorage.setItem(storageKey, "broken");
    const handle = await render();
    expect(percent(handle)).toBe(53);
    await key(handle, "ArrowRight");
    expect(percent(handle)).toBe(55);
    await key(handle, "Home");
    expect(percent(handle)).toBe(30);
    await key(handle, "End");
    expect(percent(handle)).toBe(64);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("disabled");
    });
    await key(handle, "ArrowLeft");
    expect(percent(handle)).toBe(62);
    await act(async () => handle.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    expect(percent(handle)).toBe(53);
  });

  it("cancels interrupted touch drags and keeps content mounted across settings and phone layout", async () => {
    const handle = await render();
    const terminal = host.querySelector("[data-terminal]");
    await pointer(handle, "pointerdown", 550, "touch");
    await pointer(handle, "pointermove", 350, "touch");
    expect(percent(handle)).toBe(33);
    await pointer(handle, "pointercancel", 350, "touch");
    expect(percent(handle)).toBe(53);
    expect(localStorage.getItem(storageKey)).toBeNull();
    await pointer(handle, "pointerdown", 550);
    await pointer(handle, "pointermove", 350);
    await render(true, false);
    expect(percent(handle)).toBe(53);
    expect(captured).toBeUndefined();
    expect(handle.tabIndex).toBe(-1);
    await render(false);
    expect(handle.hidden).toBe(true);
    expect(host.querySelector("[data-terminal]")).toBe(terminal);
    await render();
    expect(handle.hidden).toBe(false);
    expect(host.querySelector("[data-terminal]")).toBe(terminal);
  });

  it("fits a smaller desktop without overwriting the user's preferred larger-screen ratio", async () => {
    width = 1188;
    localStorage.setItem(storageKey, "68");
    const handle = await render();
    expect(percent(handle)).toBe(68);
    width = 988;
    await act(async () => resize());
    expect(percent(handle)).toBe(63);
    expect(localStorage.getItem(storageKey)).toBe("68");
    width = 1188;
    await act(async () => resize());
    expect(percent(handle)).toBe(68);
  });
});
