import { describe, expect, it, vi } from "vite-plus/test";

import { refreshGlobalEventsFallback } from "./globalEventFallback";

describe("global event polling fallback", () => {
  it("refreshes groups and actors while visible", () => {
    const refreshGroups = vi.fn();
    const refreshActors = vi.fn();

    refreshGlobalEventsFallback(false, refreshGroups, refreshActors);

    expect(refreshGroups).toHaveBeenCalledOnce();
    expect(refreshActors).toHaveBeenCalledOnce();
  });

  it("does not refresh while the document is hidden", () => {
    const refreshGroups = vi.fn();
    const refreshActors = vi.fn();

    refreshGlobalEventsFallback(true, refreshGroups, refreshActors);

    expect(refreshGroups).not.toHaveBeenCalled();
    expect(refreshActors).not.toHaveBeenCalled();
  });
});
