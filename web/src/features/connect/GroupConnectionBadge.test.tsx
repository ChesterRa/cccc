// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { GroupConnectionBadge } from "./GroupConnectionBadge";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("isolates badge activation from the row's mouse, touch, pointer and selection handlers", async () => {
  const host = document.createElement("div"),
    root = createRoot(host);
  const row = vi.fn(),
    open = vi.fn();
  try {
    await act(async () =>
      root.render(
        <div onMouseDown={row} onTouchStart={row} onPointerDown={row} onClick={row}>
          <GroupConnectionBadge
            connection={{ count: 1, expires_at: "2099-01-01T00:00:00Z" }}
            onClick={open}
          />
        </div>,
      ),
    );
    const badge = host.querySelector("button")!;
    for (const type of ["mousedown", "touchstart", "pointerdown"]) {
      const event = new Event(type, { bubbles: true, cancelable: true });
      await act(async () => badge.dispatchEvent(event));
      expect(event.defaultPrevented).toBe(false);
    }
    await act(async () => badge.click());
    expect(row).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});
