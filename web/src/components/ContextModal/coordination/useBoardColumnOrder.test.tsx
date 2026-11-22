// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useBoardColumnOrder } from "./useBoardColumnOrder";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
afterEach(() => vi.unstubAllGlobals());

it("orders the columns by the board's own width", () => {
  // happy-dom has no layout, so the board width comes from this observer.
  let resize = (_width: number) => {};
  let observed = 0;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        resize = (width) =>
          callback(
            [{ contentRect: { width } } as ResizeObserverEntry],
            this as unknown as ResizeObserver,
          );
      }
      observe() {
        observed += 1;
      }
      disconnect() {}
    },
  );
  function Board({ shown, archived }: { shown: boolean; archived: boolean }) {
    const board = useRef<HTMLDivElement>(null);
    const order = useBoardColumnOrder(board, shown, archived);
    return shown ? <div ref={board}>{order.join(",")}</div> : <p>{order.join(",")}</p>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    // Not shown yet: nothing to measure, and the board starts single-column.
    act(() => root.render(<Board shown={false} archived={false} />));
    expect(host.textContent).toBe("active,planned,done");
    expect(observed).toBe(0);

    act(() => root.render(<Board shown archived={false} />));
    expect(observed).toBe(1);
    act(() => resize(540));
    expect(host.textContent).toBe("planned,active,done");
    act(() => resize(539));
    expect(host.textContent).toBe("active,planned,done");

    act(() => root.render(<Board shown archived />));
    expect(host.textContent).toBe("active,planned,done,archived");
    act(() => resize(800));
    expect(host.textContent).toBe("planned,active,done,archived");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
