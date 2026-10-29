// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useKeepInlineReplyVisible } from "./useKeepInlineReplyVisible";

function Harness({ enabled }: { enabled: boolean }) {
  const scroll = useRef<HTMLDivElement>(null);
  useKeepInlineReplyVisible(scroll, enabled, 0);
  return (
    <div ref={scroll}>
      <div data-pending-reply>
        <textarea />
      </div>
    </div>
  );
}

afterEach(() => vi.useRealTimers());

it("keeps an inline editor visible when the viewport shrinks, but does not move an unfocused view", async () => {
  vi.useFakeTimers();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const originalViewport = Object.getOwnPropertyDescriptor(window, "visualViewport");
  const viewport = Object.assign(new EventTarget(), { height: 640, offsetTop: 0 });
  Object.defineProperty(window, "visualViewport", { configurable: true, value: viewport });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness enabled={false} />));
    const scroll = host.firstElementChild as HTMLDivElement;
    const editor = host.querySelector("textarea")!;
    vi.spyOn(scroll, "getBoundingClientRect").mockImplementation(
      () => ({ top: 0, bottom: viewport.height }) as DOMRect,
    );
    vi.spyOn(editor, "getBoundingClientRect").mockImplementation(
      () =>
        ({ top: 500 - scroll.scrollTop, bottom: 580 - scroll.scrollTop, height: 80 }) as DOMRect,
    );
    await act(async () => editor.focus());
    await act(async () => root.render(<Harness enabled />));
    await act(async () => vi.advanceTimersByTime(20));
    expect(scroll.scrollTop).toBe(0);

    await act(async () => {
      viewport.height = 380;
      viewport.dispatchEvent(new Event("resize"));
      vi.advanceTimersByTime(20);
    });
    expect(scroll.scrollTop).toBe(208);

    await act(async () => editor.blur());
    scroll.scrollTop = 0;
    await act(async () => {
      viewport.height = 300;
      viewport.dispatchEvent(new Event("resize"));
      vi.advanceTimersByTime(20);
    });
    expect(scroll.scrollTop).toBe(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    if (originalViewport) Object.defineProperty(window, "visualViewport", originalViewport);
    else Reflect.deleteProperty(window, "visualViewport");
  }
});
