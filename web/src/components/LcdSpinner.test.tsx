// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { LcdSpinner } from "./LcdSpinner";

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const mark = () => host.querySelector<SVGSVGElement>("svg.lcd-spinner");

it("keeps its box but shows no cells for a short wait, then steps round the grid", async () => {
  await act(async () => root.render(<LcdSpinner className="h-4 w-4" />));
  // The box is there from the start, so nothing beside it moves when the cells appear.
  expect(mark()).not.toBeNull();
  expect(mark()?.hasAttribute("data-shown")).toBe(false);
  expect(mark()?.getAttribute("aria-hidden")).toBe("true");
  await act(async () => vi.advanceTimersByTime(249));
  expect(mark()?.hasAttribute("data-shown")).toBe(false);
  await act(async () => vi.advanceTimersByTime(1));
  expect(mark()?.hasAttribute("data-shown")).toBe(true);
  // Eight outer cells, each lighting one 125 ms step after the one before it.
  const delays = [...mark()!.querySelectorAll("rect")].map((cell) => cell.style.animationDelay);
  expect(delays).toEqual([
    "0ms",
    "-875ms",
    "-750ms",
    "-625ms",
    "-500ms",
    "-375ms",
    "-250ms",
    "-125ms",
  ]);
});

it("keeps the caller's own icon until the delay passes", async () => {
  await act(async () =>
    root.render(
      <LcdSpinner>
        <i data-own-icon />
      </LcdSpinner>,
    ),
  );
  expect(host.querySelector("[data-own-icon]")).not.toBeNull();
  expect(mark()).toBeNull();
  await act(async () => vi.advanceTimersByTime(250));
  expect(host.querySelector("[data-own-icon]")).toBeNull();
  expect(mark()?.hasAttribute("data-shown")).toBe(true);
});

it("ends the wait without a flash when the work finishes first", async () => {
  await act(async () =>
    root.render(
      <LcdSpinner>
        <i data-own-icon />
      </LcdSpinner>,
    ),
  );
  await act(async () => vi.advanceTimersByTime(100));
  await act(async () => root.render(<i data-own-icon />));
  await act(async () => vi.advanceTimersByTime(500));
  expect(mark()).toBeNull();
  expect(host.querySelector("[data-own-icon]")).not.toBeNull();
});
