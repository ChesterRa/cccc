// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../../stores/useUIStore";
import { useComposerHeightResize } from "./useComposerHeightResize";
import { useComposerTextareaAutoResize } from "./useComposerTextareaAutoResize";
import { ComposerResizeHandle } from "./ComposerResizeHandle";

const longText = Array(30).fill("line").join("\n");
function Probe({
  value = longText,
  enabled = true,
  scale = 1,
}: {
  value?: string;
  enabled?: boolean;
  scale?: number;
}) {
  const footerRef = useRef<HTMLElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const resize = useComposerHeightResize({ footerRef, composerRef, enabled, scale });
  useComposerTextareaAutoResize({
    composerRef,
    value,
    minHeight: 64 * scale,
    maxHeight: resize.height,
  });
  return (
    <section data-panel>
      <main />
      <footer ref={footerRef}>
        {enabled ? <ComposerResizeHandle {...resize} label="Input height limit" /> : null}
        <textarea
          ref={composerRef}
          value={value}
          readOnly
          style={{ maxHeight: "var(--composer-max-height)" }}
        />
      </footer>
    </section>
  );
}

describe("composer height interaction", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let panelHeight: number;
  let stopListening: () => void;
  const updates = vi.fn();
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    panelHeight = 800;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    useUIStore.setState({ composerHeight: 64 });
    updates.mockClear();
    stopListening = useUIStore.subscribe(updates);
    vi.spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get").mockImplementation(
      function (this: HTMLTextAreaElement) {
        return this.value.split("\n").length * 20 + 24;
      },
    );
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      function (this: Element) {
        const height = parseFloat(host.querySelector("textarea")?.style.height || "64");
        const h = this.hasAttribute("data-panel")
          ? panelHeight
          : this.tagName === "MAIN"
            ? panelHeight - 100 - height
            : this.tagName === "FOOTER"
              ? height + 100
              : this.tagName === "TEXTAREA"
                ? height
                : 12;
        return {
          x: 0,
          y: 0,
          width: 1000,
          height: h,
          top: 0,
          left: 0,
          right: 1000,
          bottom: h,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    stopListening();
    host.remove();
    vi.restoreAllMocks();
  });
  async function render(props = {}) {
    await act(async () => root.render(<Probe {...props} />));
  }
  async function pointer(type: string, y: number, target: EventTarget = window) {
    await act(async () =>
      target.dispatchEvent(
        new PointerEvent(type, {
          clientY: y,
          pointerId: 1,
          button: 0,
          isPrimary: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
  }
  it("previews content growth without store writes, commits once, and shrinks empty drafts", async () => {
    await render();
    const storage = vi.spyOn(Storage.prototype, "setItem");
    const handle = host.querySelector('[role="separator"]')!;
    await pointer("pointerdown", 600, handle);
    await pointer("pointermove", 500);
    await pointer("pointermove", 400);
    expect(host.querySelector("textarea")!.style.height).toBe("264px");
    expect(handle.getAttribute("aria-valuenow")).toBe("264");
    expect(updates).not.toHaveBeenCalled();
    expect(storage).not.toHaveBeenCalled();
    await pointer("pointerup", 400);
    expect(updates).toHaveBeenCalledOnce();
    expect(storage).toHaveBeenCalledOnce();
    expect(useUIStore.getState().composerHeight).toBe(264);
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
    await render({ value: "" });
    expect(host.querySelector("textarea")!.style.height).toBe("64px");
    await render({ value: "a\nb\nc\nd\ne" });
    expect(host.querySelector("textarea")!.style.height).toBe("124px");
  });
  it("supports keyboard bounds, resize clamping, font scale, and double-click reset", async () => {
    await render({ scale: 1.25 });
    const handle = host.querySelector('[role="separator"]')!;
    const key = async (value: string) => {
      await act(async () =>
        handle.dispatchEvent(new KeyboardEvent("keydown", { key: value, bubbles: true })),
      );
    };
    await key("ArrowUp");
    expect(useUIStore.getState().composerHeight).toBe(80);
    expect(handle.getAttribute("aria-valuenow")).toBe("100");
    await key("ArrowDown");
    expect(useUIStore.getState().composerHeight).toBe(64);
    await key("End");
    expect(useUIStore.getState().composerHeight).toBe(384);
    panelHeight = 350;
    await act(async () => window.dispatchEvent(new Event("resize")));
    expect(handle.getAttribute("aria-valuenow")).toBe("150");
    expect(host.querySelector("textarea")!.style.height).toBe("150px");
    await key("Home");
    expect(useUIStore.getState().composerHeight).toBe(64);
    await key("End");
    await act(async () => handle.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    expect(useUIStore.getState().composerHeight).toBe(64);
  });
  it("cancels a drag without persisting and restores existing body styles", async () => {
    await render();
    document.body.style.cursor = "crosshair";
    document.body.style.userSelect = "text";
    await pointer("pointerdown", 600, host.querySelector('[role="separator"]')!);
    await pointer("pointermove", 300);
    await pointer("pointercancel", 300);
    expect(updates).not.toHaveBeenCalled();
    expect(host.querySelector("textarea")!.style.height).toBe("64px");
    expect(document.body.style.cursor).toBe("crosshair");
    expect(document.body.style.userSelect).toBe("text");
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  });
  it("does not expose or activate a mobile drag target", async () => {
    await render({ enabled: false });
    expect(host.querySelector('[role="separator"]')).toBeNull();
    await pointer("pointermove", 400);
    await pointer("pointerup", 400);
    expect(updates).not.toHaveBeenCalled();
    expect(document.body.style.cursor).toBe("");
  });
});
