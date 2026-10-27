// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useRuntimeDockTickerEntries } from "./useRuntimeDockTickerEntries";
import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let group = 0;
const hiddenDescriptor = Object.getOwnPropertyDescriptor(document, "hidden");
const now = 100000;
const entry = (text = "working", receivedAt = now): RuntimeDockTickerEntry => ({
  id: text,
  actorId: "alpha",
  actorLabel: "Alpha",
  kind: "message",
  text,
  receivedAt,
  updatedAt: "",
  sourceId: text,
});
function Probe({
  entries,
  suppressed = false,
}: {
  entries: RuntimeDockTickerEntry[];
  suppressed?: boolean;
}) {
  const visible = useRuntimeDockTickerEntries(`ticker-test-${group}`, entries, suppressed);
  return <div>{visible.map((e) => e.text).join("|")}</div>;
}
async function render(entries: RuntimeDockTickerEntry[], suppressed = false) {
  await act(async () => root.render(<Probe entries={entries} suppressed={suppressed} />));
  await act(async () => vi.advanceTimersByTime(0));
}
async function hide(value: boolean) {
  await act(async () => {
    Object.defineProperty(document, "hidden", { configurable: true, value });
    document.dispatchEvent(new Event("visibilitychange"));
  });
}
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.setSystemTime(now);
  group++;
  Object.defineProperty(document, "hidden", { configurable: true, value: false });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  if (hiddenDescriptor) Object.defineProperty(document, "hidden", hiddenDescriptor);
  else Reflect.deleteProperty(document, "hidden");
  vi.restoreAllMocks();
  vi.useRealTimers();
});
it("does not poll while text is held, and removes it exactly at its expiry", async () => {
  const timeout = vi.spyOn(window, "setTimeout");
  const interval = vi.spyOn(window, "setInterval");
  await render([entry()]);
  expect(host.textContent).toBe("working");
  timeout.mockClear();
  await act(async () => vi.advanceTimersByTime(5999));
  expect(host.textContent).toBe("working");
  expect(timeout).not.toHaveBeenCalled();
  expect(interval).not.toHaveBeenCalled();
  await act(async () => vi.advanceTimersByTime(1));
  expect(host.textContent).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});
it("cancels background timers and never revives expired text on return", async () => {
  await render([entry()]);
  await hide(true);
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => vi.advanceTimersByTime(10000));
  await hide(false);
  expect(host.textContent).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});
it("projects fresh background arrivals on return without extending their lifetime", async () => {
  await hide(true);
  await render([entry()]);
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => vi.advanceTimersByTime(2000));
  await render([entry("new", now + 2000)]);
  await act(async () => vi.advanceTimersByTime(3000));
  await hide(false);
  expect(host.textContent).toBe("new");
  await act(async () => vi.advanceTimersByTime(3000));
  expect(host.textContent).toBe("");
});
it("pauses suppressed UI, reschedules changed entries, and cleans up on unmount", async () => {
  await render([entry()]);
  await act(async () => vi.advanceTimersByTime(2000));
  await render([entry("new", now + 2000)]);
  await render([entry("new", now + 2000)], true);
  expect(host.textContent).toBe("");
  expect(vi.getTimerCount()).toBe(0);
  await render([entry("new", now + 2000)]);
  await act(async () => vi.advanceTimersByTime(4000));
  expect(host.textContent).toBe("new");
  await act(async () => root.render(null));
  expect(vi.getTimerCount()).toBe(0);
});
