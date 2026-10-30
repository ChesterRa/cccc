// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useScrollAnchorRestoration } from "./useScrollAnchorRestoration";

describe("useScrollAnchorRestoration lifecycle", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });

  it("releases forced bottom follow before applying a restored anchor", async () => {
    let forcedBottom = true;
    const releaseForcedBottom = vi.fn(() => {
      forcedBottom = false;
    });
    const applyAnchor = vi.fn(() => {
      expect(forcedBottom).toBe(false);
      return true;
    });
    let begin: ReturnType<typeof useScrollAnchorRestoration>["begin"] | undefined;

    function Probe() {
      begin = useScrollAnchorRestoration(applyAnchor, releaseForcedBottom).begin;
      return null;
    }

    await act(async () => root.render(<Probe />));
    act(() => {
      expect(begin?.({ anchorId: "event-1", offsetPx: 24 })).toBe(true);
    });

    expect(releaseForcedBottom).toHaveBeenCalledOnce();
    expect(releaseForcedBottom.mock.invocationCallOrder[0]).toBeLessThan(
      applyAnchor.mock.invocationCallOrder[0],
    );
  });

  it("extends only an active restoration, by a full window from the latest reflow", async () => {
    let now = 1_000;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let restoration: ReturnType<typeof useScrollAnchorRestoration> | undefined;

    function Probe() {
      restoration = useScrollAnchorRestoration(() => true);
      return null;
    }

    await act(async () => root.render(<Probe />));
    expect(restoration?.extend()).toBe(false);

    act(() => {
      restoration?.begin({ anchorId: "event-1", offsetPx: 0 });
    });
    now += 4_000;
    expect(restoration?.extend()).toBe(true);
    now += 4_000;
    expect(restoration?.isActive()).toBe(true);
    now += 1_500;
    expect(restoration?.isActive()).toBe(false);
    expect(restoration?.extend()).toBe(false);
  });
});
