// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { TaskDetailLayout } from "./TaskDetailLayout";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const WIDTH_KEY = "cccc-task-detail-width";

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.removeItem(WIDTH_KEY);
});

it("keeps the preferred detail width while the container clamps it or switches to one pane", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const scroll = createRef<HTMLDivElement>();
  let measuredWidth = 2_000;
  let resized = () => {};

  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      disconnect() {}
    },
  );

  try {
    await act(async () => {
      root.render(
        <TaskDetailLayout
          scrollRef={scroll}
          isDark={false}
          detailLabel="Task editor"
          resizeLabel="Resize task details"
          renderDetail={() => <button type="button">Close</button>}
        >
          <button type="button">Task A</button>
        </TaskDetailLayout>,
      );
    });
    const layout = host.querySelector<HTMLElement>("[data-task-detail-layout]")!;
    Object.defineProperty(layout, "clientWidth", { get: () => measuredWidth });
    const setWidth = async (width: number) => {
      measuredWidth = width;
      await act(async () => resized());
    };
    const divider = () => layout.querySelector<HTMLElement>('[role="separator"]');
    const shownWidth = () => Number(divider()?.getAttribute("aria-valuenow"));

    await setWidth(2_000);
    expect(layout.dataset.taskDetailLayout).toBe("split");
    expect(shownWidth()).toBe(800);
    await act(async () => {
      divider()?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    });
    expect(shownWidth()).toBe(824);
    expect(localStorage.getItem(WIDTH_KEY)).toBe("824");

    await setWidth(1_100);
    expect(layout.dataset.taskDetailLayout).toBe("split");
    expect(shownWidth()).toBe(492);
    expect(localStorage.getItem(WIDTH_KEY)).toBe("824");
    await act(async () => {
      divider()?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    });
    expect(shownWidth()).toBe(492);
    expect(localStorage.getItem(WIDTH_KEY)).toBe("824");

    const board = scroll.current;
    await setWidth(1_000);
    expect(layout.dataset.taskDetailLayout).toBe("detail");
    expect(divider()).toBeNull();
    expect(scroll.current).toBe(board);
    expect(board?.hasAttribute("inert")).toBe(true);

    await setWidth(2_000);
    expect(layout.dataset.taskDetailLayout).toBe("split");
    expect(board?.hasAttribute("inert")).toBe(false);
    expect(shownWidth()).toBe(824);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
