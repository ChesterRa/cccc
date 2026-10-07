import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { createTerminalResizeCoalescer } from "./terminalResizeCoalescer";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it("forwards only the settled size while the keyboard slides through intermediate sizes", () => {
  const sent: Array<[number, number]> = [];
  const coalescer = createTerminalResizeCoalescer((cols, rows) => sent.push([cols, rows]), 150);

  // Keyboard animation: the terminal is refit every frame on the way down.
  for (const rows of [46, 42, 36, 30, 25, 20]) {
    coalescer.push(47, rows);
    vi.advanceTimersByTime(16);
  }
  expect(sent).toEqual([]);

  vi.advanceTimersByTime(150);
  // One repaint for the final size instead of one per intermediate frame.
  expect(sent).toEqual([[47, 20]]);
});

it("still forwards each resize once the size has settled in between", () => {
  const sent: Array<[number, number]> = [];
  const coalescer = createTerminalResizeCoalescer((cols, rows) => sent.push([cols, rows]), 150);
  coalescer.push(80, 24);
  vi.advanceTimersByTime(150);
  coalescer.push(100, 30);
  vi.advanceTimersByTime(150);
  expect(sent).toEqual([
    [80, 24],
    [100, 30],
  ]);
});

it("drops a pending resize when the connection is disposed", () => {
  const sent: Array<[number, number]> = [];
  const coalescer = createTerminalResizeCoalescer((cols, rows) => sent.push([cols, rows]), 150);
  coalescer.push(80, 24);
  coalescer.dispose();
  vi.advanceTimersByTime(500);
  expect(sent).toEqual([]);
});
