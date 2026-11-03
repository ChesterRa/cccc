// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { DOCK_HEIGHT_PROPERTY, useFloatingDockReserve } from "./useFloatingDockReserve";

let resize: (() => void) | null = null;
class FakeResizeObserver {
  constructor(callback: () => void) {
    resize = callback;
  }
  observe() {}
  disconnect() {
    resize = null;
  }
}

function Harness({
  dock,
  list,
  enabled,
}: {
  dock: HTMLElement;
  list: HTMLElement;
  enabled: boolean;
}) {
  const scrollRef = useRef<HTMLElement | null>(list);
  useFloatingDockReserve(dock, scrollRef, enabled);
  return null;
}

function fixture() {
  const view = document.createElement("div");
  view.setAttribute("data-group-message-view", "");
  const dock = document.createElement("div");
  const list = document.createElement("div");
  view.append(list, dock);
  document.body.append(view);
  let dockHeight = 60;
  dock.getBoundingClientRect = () => ({ height: dockHeight }) as DOMRect;
  // A list of 2000px in a 600px window; the room at its end grows with the dock.
  let top = 1400;
  Object.defineProperties(list, {
    clientHeight: { get: () => 600 },
    scrollHeight: {
      get: () => {
        const room = Number.parseInt(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY) || "0");
        return 1928 + Math.max(72, room + 12);
      },
    },
    scrollTop: {
      get: () => top,
      set: (value: number) => {
        top = Math.min(value, list.scrollHeight - list.clientHeight);
      },
    },
  });
  const host = document.createElement("div");
  const root = createRoot(host);
  return {
    view,
    dock,
    list,
    setDockHeight: (height: number) => {
      dockHeight = height;
    },
    render: (enabled = true) =>
      act(async () => root.render(<Harness dock={dock} list={list} enabled={enabled} />)),
    unmount: () => act(async () => root.unmount()),
  };
}

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  resize = null;
});

it("gives the list room for members a line of activity made taller, keeping the newest in view", async () => {
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  const { view, list, setDockHeight, render, unmount } = fixture();
  await render();
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("60px");
  expect(list.scrollTop).toBe(1400);

  setDockHeight(92);
  act(() => resize?.());
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("92px");
  // The list was at its newest message, so it follows the larger room.
  expect(list.scrollTop).toBe(list.scrollHeight - list.clientHeight);

  // Leaving phone width clears the height, so no wider layout inherits it.
  await render(false);
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("");
  await unmount();
});

it("leaves a reader of earlier messages in place and keeps the room while the dock hides", async () => {
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  const { view, list, setDockHeight, render, unmount } = fixture();
  await render();
  list.scrollTop = 900;

  setDockHeight(92);
  act(() => resize?.());
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("92px");
  expect(list.scrollTop).toBe(900);

  // Reading mode or the keyboard hides the dock: it measures nothing and the room stays.
  setDockHeight(0);
  act(() => resize?.());
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("92px");

  // Shown again without the activity line, it is measured afresh.
  setDockHeight(60);
  act(() => resize?.());
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("60px");
  await unmount();
  expect(view.style.getPropertyValue(DOCK_HEIGHT_PROPERTY)).toBe("");
});
