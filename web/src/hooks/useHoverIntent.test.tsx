// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { isMousePointer, useHoverIntent } from "./useHoverIntent";

type Api = ReturnType<typeof useHoverIntent>;

let root: Root;
let host: HTMLDivElement;
let api: Api;
const setOpen = vi.fn();

function Probe({ openDelayMs, closeDelayMs }: { openDelayMs?: number; closeDelayMs?: number }) {
  api = useHoverIntent(setOpen, { openDelayMs, closeDelayMs });
  return null;
}

async function mount(options: { openDelayMs?: number; closeDelayMs?: number } = {}) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Probe {...options} />));
}

beforeEach(() => {
  vi.useFakeTimers();
  setOpen.mockClear();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe("useHoverIntent", () => {
  it("waits out the open delay so a passing pointer does not flash the menu", async () => {
    await mount({ openDelayMs: 120 });

    act(() => api.scheduleOpen());
    act(() => void vi.advanceTimersByTime(119));
    expect(setOpen).not.toHaveBeenCalled();

    act(() => void vi.advanceTimersByTime(1));
    expect(setOpen).toHaveBeenCalledWith(true);
  });

  it("drops a pending open when the pointer leaves first", async () => {
    await mount({ openDelayMs: 120, closeDelayMs: 220 });

    act(() => api.scheduleOpen());
    act(() => void vi.advanceTimersByTime(60));
    act(() => api.scheduleClose());
    act(() => void vi.advanceTimersByTime(500));

    // Only the close resolves: the superseded open must never fire.
    expect(setOpen).toHaveBeenCalledTimes(1);
    expect(setOpen).toHaveBeenCalledWith(false);
  });

  it("keeps the menu open when the pointer crosses into it before the close delay", async () => {
    await mount({ closeDelayMs: 220 });

    act(() => api.scheduleClose());
    act(() => void vi.advanceTimersByTime(150));
    act(() => api.cancel());
    act(() => void vi.advanceTimersByTime(500));

    expect(setOpen).not.toHaveBeenCalled();
  });

  it("stops a pending timer when the component unmounts", async () => {
    await mount({ openDelayMs: 120 });

    act(() => api.scheduleOpen());
    await act(async () => root.unmount());
    act(() => void vi.advanceTimersByTime(500));

    expect(setOpen).not.toHaveBeenCalled();

    // Keep afterEach's unmount harmless.
    host.remove();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  it("treats only a mouse as hover intent", () => {
    expect(isMousePointer({ pointerType: "mouse" })).toBe(true);
    expect(isMousePointer({ pointerType: "touch" })).toBe(false);
    expect(isMousePointer({ pointerType: "pen" })).toBe(false);
    expect(isMousePointer({})).toBe(false);
  });
});
