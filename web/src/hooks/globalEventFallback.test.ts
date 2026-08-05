import { describe, expect, it, vi } from "vite-plus/test";

import { refreshGlobalEventsFallback } from "./globalEventFallback";

describe("global event polling fallback", () => {
  it("refreshes groups while visible", () => {
    const refreshGroups = vi.fn();

    refreshGlobalEventsFallback(false, refreshGroups);

    expect(refreshGroups).toHaveBeenCalledOnce();
  });

  it("does not refresh while the document is hidden", () => {
    const refreshGroups = vi.fn();

    refreshGlobalEventsFallback(true, refreshGroups);

    expect(refreshGroups).not.toHaveBeenCalled();
  });
});
