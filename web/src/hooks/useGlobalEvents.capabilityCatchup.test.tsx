// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useGlobalEvents } from "./useGlobalEvents";

class FakeEventSource {
  static instances: FakeEventSource[] = [];

  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private listeners = new Map<string, (event: Event) => void>();

  constructor(public readonly url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(kind: string, listener: EventListener) {
    this.listeners.set(kind, listener as (event: Event) => void);
  }

  emit(kind: string, data: unknown) {
    this.listeners.get(kind)?.(new MessageEvent(kind, { data: JSON.stringify(data) }));
  }

  close() {}
}

function Probe({ refreshCapabilities }: { refreshCapabilities: (groupId: string) => void }) {
  useGlobalEvents({
    refreshGroups: () => undefined,
    refreshActors: () => undefined,
    selectedGroupId: "g-selected",
    refreshCapabilities,
  });
  return null;
}

describe("useGlobalEvents capability catch-up", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    FakeEventSource.instances = [];
    vi.stubGlobal("EventSource", FakeEventSource);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it("refreshes the selected capability catalog on first open, reconnect, and change event", async () => {
    const refreshCapabilities = vi.fn();
    await act(async () => root.render(<Probe refreshCapabilities={refreshCapabilities} />));
    const eventSource = FakeEventSource.instances[0];
    expect(eventSource?.url).toContain("/api/v1/events/stream");

    await act(async () => eventSource?.onopen?.());
    await act(async () => eventSource?.onopen?.());
    eventSource?.emit("event", { kind: "capability.changed", group_id: "g-selected" });

    expect(refreshCapabilities).toHaveBeenCalledTimes(3);
    expect(refreshCapabilities).toHaveBeenNthCalledWith(1, "g-selected");
    expect(refreshCapabilities).toHaveBeenNthCalledWith(2, "g-selected");
    expect(refreshCapabilities).toHaveBeenNthCalledWith(3, "g-selected");
  });

  it("does not poll the capability catalog while fallback retries the event stream", async () => {
    vi.useFakeTimers();
    try {
      const refreshCapabilities = vi.fn();
      await act(async () => root.render(<Probe refreshCapabilities={refreshCapabilities} />));
      const eventSource = FakeEventSource.instances[0];

      await act(async () => {
        eventSource?.onerror?.();
        eventSource?.onerror?.();
        eventSource?.onerror?.();
        await vi.advanceTimersByTimeAsync(30_000);
      });

      expect(FakeEventSource.instances.length).toBeGreaterThan(1);
      expect(refreshCapabilities).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
