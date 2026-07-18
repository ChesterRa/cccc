import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { apiJson, normalizePresentationBrowserSurfaceState } from "./base";

describe("apiJson", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("reports non-JSON HTTP failures as HTTP errors instead of parse errors", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("<html><head><title>504 Gateway Time-out</title></head></html>", {
        status: 504,
        statusText: "Gateway Time-out",
        headers: { "content-type": "text/html" },
      }),
    );

    const resp = await apiJson("/api/v1/groups/g1/send", { method: "POST" });

    expect(resp.ok).toBe(false);
    expect(resp.ok ? "" : resp.error.code).toBe("HTTP_ERROR");
    expect(resp.ok ? "" : resp.error.message).toContain("504 Gateway Time-out");
  });
});

describe("normalizePresentationBrowserSurfaceState", () => {
  it("preserves projected display ownership for the browser status UI", () => {
    const state = normalizePresentationBrowserSurfaceState({
      active: true,
      state: "ready",
      metadata: { display: ":123", display_owned: true, display_owner: "cccc_xvfb", adopted: true },
    });

    expect(state.metadata).toEqual({
      display: ":123",
      display_owned: true,
      display_owner: "cccc_xvfb",
      adopted: true,
    });
  });
});
