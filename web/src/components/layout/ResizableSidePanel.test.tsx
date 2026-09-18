// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ResizableSidePanel } from "./ResizableSidePanel";
import { useUIStore } from "../../stores/useUIStore";

it("keeps parent and panel contents out of drag renders while persisting the final width", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const readWidth = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000);
  const frames = new Map<number, FrameRequestCallback>();
  let nextFrame = 0;
  const request = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  useUIStore.setState({ chatSessions: {} });
  let parentRenders = 0,
    contentRenders = 0;
  function Content() {
    contentRenders++;
    return <input aria-label="Panel search" defaultValue="preserved" />;
  }
  function Harness() {
    parentRenders++;
    const container = useRef<HTMLDivElement>(null);
    return (
      <div ref={container} data-group-shell>
        <main>Messages</main>
        <ResizableSidePanel
          groupId="resize"
          surface="files"
          viewing={false}
          container={container}
          isDark={false}
        >
          {() => <Content />}
        </ResizableSidePanel>
      </div>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const pointer = (target: EventTarget, type: string, clientX: number) =>
    act(async () => {
      target.dispatchEvent(
        new PointerEvent(type, { clientX, button: 0, bubbles: true, cancelable: true }),
      );
    });
  try {
    await act(async () => root.render(<Harness />));
    expect(
      host
        .querySelector<HTMLElement>("[data-group-shell]")!
        .style.getPropertyValue("--group-side-panel-width"),
    ).toBe(host.querySelector<HTMLElement>("#group-side-panel")!.style.width);
    const initial = { parentRenders, contentRenders };
    const input = host.querySelector("input")!;
    input.focus();
    const divider = host.querySelector("[data-side-panel-resize]")!;
    await pointer(divider, "pointerdown", 500);
    for (const x of [450, 420, 400]) {
      await pointer(window, "pointermove", x);
      await act(async () => {
        const pending = [...frames.values()];
        frames.clear();
        pending.forEach((callback) => callback(0));
      });
    }
    expect(host.querySelector<HTMLElement>("#group-side-panel")!.style.width).toBe("460px");
    expect(divider.getAttribute("aria-valuenow")).toBe("460");
    expect(
      host
        .querySelector<HTMLElement>("[data-group-shell]")!
        .style.getPropertyValue("--group-side-panel-width"),
    ).toBe("460px");
    expect({ parentRenders, contentRenders }).toEqual(initial);
    expect(host.querySelector("input")).toBe(input);
    expect(document.activeElement).toBe(input);
    expect(useUIStore.getState().chatSessions.resize).toBeUndefined();
    await pointer(window, "pointerup", 400);
    expect(useUIStore.getState().chatSessions.resize.sidePanelWidth).toBe(460);
    expect(document.body.style.cursor).toBe("");
    expect(frames.size).toBe(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    readWidth.mockRestore();
    request.mockRestore();
    cancel.mockRestore();
    useUIStore.setState({ chatSessions: {} });
  }
});
